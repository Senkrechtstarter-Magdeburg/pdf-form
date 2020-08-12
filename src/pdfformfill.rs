use lopdf::{Document, Object, ObjectId, StringFormat, Error, Dictionary};

use std::{str, io};

use wasm_bindgen::prelude::*;
use std::path::Path;
use std::collections::VecDeque;
use wasm_bindgen::__rt::std::collections::HashMap;

bitflags! {
    struct ButtonFlags: u32 {
        const NO_TOGGLE_TO_OFF  = 1 << 14;
        const RADIO             = 1 << 15;
        const PUSHBUTTON        = 1 << 16;
        const RADIO_IN_UNISON   = 1 << 25;

    }
}

bitflags! {
    struct ChoiceFlags: u32 {
        const COBMO             = 0x20000;
        const EDIT              = 0x40000;
        const SORT              = 0x80000;
        const MULTISELECT       = 0x200000;
        const DO_NOT_SPELLCHECK = 0x800000;
        const COMMIT_ON_CHANGE  = 0x8000000;
    }
}

/// A PDF Form that contains fillable fields
///
/// Use this struct to load an existing PDF with a fillable form using the `load` method.  It will
/// analyze the PDF and identify the fields. Then you can get and set the content of the fields by
/// index.
#[wasm_bindgen]
pub struct Form {
    doc: Document,
    form_fields: HashMap<String, ObjectId>,
}

/// The possible types of fillable form fields in a PDF
#[wasm_bindgen]
#[derive(Debug)]
pub enum FieldType {
    Button,
    Radio,
    CheckBox,
    ListBox,
    ComboBox,
    Text,
}

/// The current state of a form field
#[derive(Debug)]
pub enum FieldState {
    /// Push buttons have no state
    Button,
    /// `selected` is the sigular option from `options` that is selected
    Radio { selected: String, options: Vec<String> },
    /// The toggle state of the checkbox
    CheckBox { is_checked: bool },
    /// `selected` is the list of selected options from `options`
    ListBox { selected: Vec<String>, options: Vec<String>, multiselect: bool },
    /// `selected` is the list of selected options from `options`
    ComboBox { selected: Vec<String>, options: Vec<String>, multiselect: bool },
    /// User Text Input
    Text { text: String },
}

#[derive(Debug, Error)]
/// Errors that may occur while loading a PDF
pub enum LoadError {
    /// An IO Error
    IoError(io::Error),
    /// A dictionary key that must be present in order to find forms was not present
    DictionaryKeyNotFound,
    /// The reference `ObjectId` did not point to any values
    #[error(non_std, no_from)]
    NoSuchReference(ObjectId),
    /// An element that was expected to be a reference was not a reference
    NotAReference,
    /// A value that must be a certain type was not that type
    UnexpectedType,
}

impl From<lopdf::Error> for LoadError {
    fn from(_: Error) -> Self {
        LoadError::UnexpectedType
    }
}

/// Errors That may occur while setting values in a form
#[wasm_bindgen]
#[derive(Debug, Error)]
#[repr(u8)]
pub enum ValueError {
    /// The method used to set the state is incompatible with the type of the field
    TypeMismatch = 0,
    /// One or more selected values are not valid choices
    InvalidSelection = 1,
    /// Multiple values were selected when only one was allowed
    TooManySelected = 2,
}

trait PdfObjectDeref {
    fn deref<'a>(&self, doc: &'a Document) -> Result<&'a Object, LoadError>;
}

impl PdfObjectDeref for Object {
    fn deref<'a>(&self, doc: &'a Document) -> Result<&'a Object, LoadError> {
        match self {
            &Object::Reference(oid) => doc.objects.get(&oid).ok_or(LoadError::NoSuchReference(oid)),
            _ => Err(LoadError::NotAReference)
        }
    }
}

impl Form {
    /// Takes a reader containing a PDF with a fillable form, analyzes the content, and attempts to
    /// identify all of the fields the form has.
    pub fn load_from<R: io::Read>(reader: R) -> Result<Self, LoadError> {
        let doc = Document::load_from(reader)?;
        Self::load_doc(doc)
    }

    /// Takes a path to a PDF with a fillable form, analyzes the file, and attempts to identify all
    /// of the fields the form has.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, LoadError> {
        let doc = Document::load(path)?;
        Self::load_doc(doc)
    }

    fn load_doc(doc: Document) -> Result<Self, LoadError> {
        let mut queue = VecDeque::new();
        let mut map: HashMap<String, ObjectId> = HashMap::new();
        // Block so borrow of doc ends before doc is moved into the result
        {
            // Get the form's top level fields
            let catalog = doc.trailer.get(b"Root")
                .or(Err(LoadError::DictionaryKeyNotFound))?
                .deref(&doc)?
                .as_dict().or(Err(LoadError::UnexpectedType))?;
            let acroform = catalog.get(b"AcroForm")
                .or(Err(LoadError::DictionaryKeyNotFound))?
                .deref(&doc)?
                .as_dict().or(Err(LoadError::UnexpectedType))?;
            let fields_list = acroform.get(b"Fields")
                .or(Err(LoadError::DictionaryKeyNotFound))?
                //    .deref(&doc)?
                .as_array().or(Err(LoadError::UnexpectedType))?;
            queue.append(&mut VecDeque::from(fields_list.clone()));

            // Iterate over the fields
            while let Some(objref) = queue.pop_front() {
                let obj = objref.deref(&doc)?;
                if let &Object::Dictionary(ref dict) = obj {
                    // If the field has FT, it actually takes input.  Save this
                    if dict.get(b"FT").is_ok() {
                        let field_id = objref.as_reference().unwrap();

                        if let Ok(Object::String(ref string_u8, _)) = dict.get(b"T") {
                            let name = Form::get_form_name(string_u8.clone())?;
                            map.insert(name, field_id);
                        }
                    }
                    // If this field has kids, they might have FT, so add them to the queue
                    if let Ok(&Object::Array(ref kids)) = dict.get(b"Kids") {
                        queue.append(&mut VecDeque::from(kids.clone()));
                    }
                }
            }
        }
        Ok(Form { doc, form_fields: map })
    }

    fn get_form_name(string_u8: Vec<u8>) -> Result<String, LoadError> {
        // Assuming the string is UTF16. First 2 Bytes indicate UTF16, so we skip them
        // Converting 8bit array to 16bit array
        let mut string_u16 = vec![0; string_u8.len()];
        for (i, byte) in string_u8.iter().skip(2).enumerate() {
            string_u16[i / 2] = if i % 2 == 0 { (u16::from(*byte)) << 8 } else { u16::from(*byte) };
        }

        // The first \0 indicates the end of a string
        let str_end = string_u16.iter().position(|x| *x == 0 as u16).unwrap_or(string_u16.len());

        if let Ok(ref name) = String::from_utf16(&string_u16[..str_end]) {
            return Ok(name.into());
        }

        Err(LoadError::UnexpectedType)
    }

    /// Returns the number of fields the form has
    pub fn len(&self) -> usize {
        self.form_fields.len()
    }

    /// Gets the type of field of the given index
    ///
    /// # Panics
    /// This function will panic if the index is greater than the number of fields
    pub fn get_type(&self, name: &String) -> FieldType {
        // unwraps should be fine because load should have verified everything exists
        let field = self.doc.objects.get(&self.form_fields.get(name.as_str()).unwrap()).unwrap().as_dict().unwrap();
        let obj_zero = Object::Integer(0);
        let type_str = field.get(b"FT").unwrap().as_name_str().unwrap();
        if type_str == "Btn" {
            let flags = ButtonFlags::from_bits_truncate(field.get(b"Ff").unwrap_or(&obj_zero).as_i64().unwrap() as u32);
            if flags.intersects(ButtonFlags::RADIO) {
                FieldType::Radio
            } else if flags.intersects(ButtonFlags::PUSHBUTTON) {
                FieldType::Button
            } else {
                FieldType::CheckBox
            }
        } else if type_str == "Ch" {
            let flags = ChoiceFlags::from_bits_truncate(field.get(b"Ff").unwrap_or(&obj_zero).as_i64().unwrap() as u32);
            if flags.intersects(ChoiceFlags::COBMO) {
                FieldType::ComboBox
            } else {
                FieldType::ListBox
            }
        } else {
            FieldType::Text
        }
    }

    /// Gets the types of all of the fields in the form
    pub fn get_all_types(&self) -> Vec<FieldType> {
        self.form_fields.keys().cloned().map(|f| self.get_type(&f)).collect::<Vec<FieldType>>()
    }

    /// Gets the state of field of the given index
    ///
    /// # Panics
    /// This function will panic if the index is greater than the number of fields
    pub fn get_state(&self, name: &String) -> FieldState {
        let field_id = self.form_fields.get(name).unwrap();
        let field = self.doc.objects.get(&field_id).unwrap().as_dict().unwrap();
        match self.get_type(name) {
            FieldType::Button => FieldState::Button,
            FieldType::Radio => FieldState::Radio {
                selected: match field.get(b"V") {
                    Ok(name) => name.as_name_str().unwrap().to_owned(),
                    Err(_) => match field.get(b"AS") {
                        Ok(name) => name.as_name_str().unwrap().to_owned(),
                        Err(_) => "".to_owned(),
                    },
                },
                options: self.get_possibilities(field_id.clone()),
            },
            FieldType::CheckBox => FieldState::CheckBox {
                is_checked:
                match field.get(b"V") {
                    Ok(name) => if name.as_name_str().unwrap() == "Yes" { true } else { false },
                    Err(_) => match field.get(b"AS") {
                        Ok(name) => if name.as_name_str().unwrap() == "Yes" { true } else { false },
                        Err(_) => false
                    }
                }
            },
            FieldType::ListBox => FieldState::ListBox {
                // V field in a list box can be either text for one option, an array for many
                // options, or null
                selected: match field.get(b"V") {
                    Ok(selection) => match selection {
                        &Object::String(ref s, StringFormat::Literal) => vec![str::from_utf8(&s).unwrap().to_owned()],
                        &Object::Array(ref chosen) => {
                            let mut res = Vec::new();
                            for obj in chosen {
                                if let &Object::String(ref s, StringFormat::Literal) = obj {
                                    res.push(str::from_utf8(&s).unwrap().to_owned());
                                }
                            }
                            res
                        }
                        _ => Vec::new()
                    },
                    Err(_) => Vec::new(),
                },
                // The options is an array of either text elements or arrays where the second
                // element is what we want
                options: match field.get(b"Opt") {
                    Ok(&Object::Array(ref options)) => options.iter().map(|x| {
                        match x {
                            &Object::String(ref s, StringFormat::Literal) => str::from_utf8(&s).unwrap().to_owned(),
                            &Object::Array(ref arr) => if let &Object::String(ref s, StringFormat::Literal) = &arr[1] {
                                str::from_utf8(&s).unwrap().to_owned()
                            } else {
                                String::new()
                            },
                            _ => String::new()
                        }
                    }).filter(|x| x.len() > 0).collect(),
                    _ => Vec::new()
                },
                multiselect: {
                    let flags = ChoiceFlags::from_bits_truncate(field.get(b"Ff").unwrap().as_i64().unwrap() as u32);
                    flags.intersects(ChoiceFlags::MULTISELECT)
                },
            },
            FieldType::ComboBox => FieldState::ComboBox {
                // V field in a list box can be either text for one option, an array for many
                // options, or null
                selected: match field.get(b"V") {
                    Ok(selection) => match selection {
                        &Object::String(ref s, StringFormat::Literal) => vec![str::from_utf8(&s).unwrap().to_owned()],
                        &Object::Array(ref chosen) => {
                            let mut res = Vec::new();
                            for obj in chosen {
                                if let &Object::String(ref s, StringFormat::Literal) = obj {
                                    res.push(str::from_utf8(&s).unwrap().to_owned());
                                }
                            }
                            res
                        }
                        _ => Vec::new()
                    },
                    Err(_) => Vec::new(),
                },
                // The options is an array of either text elements or arrays where the second
                // element is what we want
                options: match field.get(b"Opt") {
                    Ok(&Object::Array(ref options)) => options.iter().map(|x| {
                        match x {
                            &Object::String(ref s, StringFormat::Literal) => str::from_utf8(&s).unwrap().to_owned(),
                            &Object::Array(ref arr) => if let &Object::String(ref s, StringFormat::Literal) = &arr[1] {
                                str::from_utf8(&s).unwrap().to_owned()
                            } else {
                                String::new()
                            },
                            _ => String::new()
                        }
                    }).filter(|x| x.len() > 0).collect(),
                    _ => Vec::new()
                },
                multiselect: {
                    let flags = ChoiceFlags::from_bits_truncate(field.get(b"Ff").unwrap().as_i64().unwrap() as u32);
                    flags.intersects(ChoiceFlags::MULTISELECT)
                },
            },
            FieldType::Text => FieldState::Text {
                text:
                match field.get(b"V") {
                    Ok(&Object::String(ref s, StringFormat::Literal)) =>
                        str::from_utf8(&s.clone()).unwrap().to_owned(),
                    _ => "".to_owned()
                }
            }
        }
    }

    fn get_possibilities(&self, oid: ObjectId) -> Vec<String> {
        let mut res = Vec::new();
        let kids_obj = self.doc.objects.get(&oid).unwrap().as_dict().unwrap().get(b"Kids");
        if let Ok(&Object::Array(ref kids)) = kids_obj {
            for kid in kids {
                let options_dict = kid.deref(&self.doc).unwrap().as_dict().unwrap().get(b"AP").unwrap().as_dict().unwrap().get(b"N").unwrap().as_dict().unwrap();
                for (key, _val) in options_dict {
                    res.push(String::from_utf8(key.to_owned()).unwrap());
                }
            }
        }
        res
    }

    /// If the field at index `n` is a text field, fills in that field with the text `s`.
    /// If it is not a text field, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_text(&mut self, name: &String, s: String) -> Result<(), JsValue> {
        match self.get_type(name) {
            FieldType::Text => {
                let field = self.doc.objects.get_mut(&self.form_fields[name]).unwrap().as_dict_mut().unwrap();
                field.set("V", Object::String(s.into_bytes(), StringFormat::Literal));
                field.remove(b"AP");
                Ok(())
            }
            _ => Err(JsValue::from(ValueError::TypeMismatch as u8))
        }
    }

    /// If the field at index `n` is a radio field, toggles the radio button based on the value
    /// `choice`
    /// If it is not a radio button field or the choice is not a valid option, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_radio(&mut self, name: &String, choice: String) -> Result<(), JsValue> {
        let field_id = self.form_fields.get(name).unwrap();
        match self.get_state(name) {
            FieldState::Radio { selected: _, options } => if options.contains(&choice) {
                let mut doc_objects = self.doc.objects.clone();
                let field = doc_objects.get_mut(&field_id).unwrap().as_dict_mut().unwrap();
                let kids = field.get_mut(b"Kids").unwrap().as_array_mut().unwrap();
                for kid in kids {
                    let kid_reference = self.doc.objects.get_mut(&kid.as_reference().unwrap()).unwrap();
                    let kid_dict = kid_reference.as_dict_mut().unwrap();
                    let kid_options_dict = kid_dict.get_mut(b"AP").unwrap().as_dict_mut().unwrap().get_mut(b"N").unwrap().as_dict_mut().unwrap();
                    if kid_options_dict.has(&choice.as_bytes()) {
                        kid_dict.set("AS", Object::Name(choice.clone().into_bytes()));
                    } else {
                        kid_dict.set("AS", Object::Name(String::from("Off").into_bytes()));
                    }
                }

                field.set("V", Object::Name(choice.into_bytes()));
                Ok(())
            } else {
                Err(JsValue::from(ValueError::InvalidSelection as u8))
            },
            _ => Err(JsValue::from(ValueError::TypeMismatch as u8))
        }
    }


    /// If the field at index `n` is a checkbox field, toggles the check box based on the value
    /// `is_checked`.
    /// If it is not a checkbox field, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_check_box(&mut self, name: &String, is_checked: bool) -> Result<(), JsValue> {
        match self.get_type(name) {
            FieldType::CheckBox => {
                let state = Object::Name({ if is_checked { "Yes" } else { "Off" } }.to_owned().into_bytes());
                let field = self.doc.objects.get_mut(&self.form_fields.get(name).unwrap()).unwrap().as_dict_mut().unwrap();
                field.set("V", state.clone());
                field.set("AS", state);
                Ok(())
            }
            _ => Err(JsValue::from(ValueError::TypeMismatch as u8))
        }
    }


    /// If the field at index `n` is a listbox or comboox field, selects the options in `choice`
    /// If it is not a listbox or combobox field or one of the choices is not a valid option, or if too many choices are selected, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_choice(&mut self, name: &String, choices: Vec<String>) -> Result<(), ValueError> {
        let field_id = self.form_fields.get(name).unwrap();
        match self.get_state(name) {
            FieldState::ListBox { selected: _, options, multiselect } | FieldState::ComboBox { selected: _, options, multiselect } => if choices.iter().fold(true, |a, h| options.contains(h) && a) {
                if !multiselect && choices.len() > 1 {
                    Err(ValueError::TooManySelected)
                } else {
                    let field = self.doc.objects.get_mut(&field_id).unwrap().as_dict_mut().unwrap();
                    match choices.len() {
                        0 => field.set("V", Object::Null),
                        1 => field.set("V", Object::String(choices[0].clone().into_bytes(),
                                                           StringFormat::Literal)),
                        _ => field.set("V", Object::Array(choices.iter().map(|x| Object::String(x.clone().into_bytes(), StringFormat::Literal)).collect()))
                    };
                    Ok(())
                }
            } else {
                Err(ValueError::InvalidSelection)
            },
            _ => Err(ValueError::TypeMismatch)
        }
    }

    pub fn get_field_by_name(&self, name: String) -> &Dictionary {
        self.doc.objects.get(self.form_fields.get(name.as_str()).unwrap()).unwrap().as_dict().unwrap()
    }

    pub fn get_field_names(&self) -> Vec<String> {
        self.form_fields.keys().cloned().collect::<Vec<String>>()
    }

    /// Fills the formula
    ///
    /// # Panics!
    pub fn fill(&mut self, fields: HashMap<String, String>) {
        for (key, value) in fields {
            match self.get_type(&key) {
                FieldType::Radio => {
                    self.set_radio(&key, value).unwrap();
                }
                FieldType::CheckBox => {
                    self.set_check_box(&key, value.to_lowercase().eq("true")).unwrap();
                }
                FieldType::Text => {
                    self.set_text(&key, value).unwrap();
                }
                _ => {}
            };
        }
    }


    /// Saves the form to the specified path
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<(), io::Error> {
        self.doc.save(path).map(|_| ())
    }

    /// Saves the form to the specified path
    pub fn save_to<W: io::Write>(&mut self, target: &mut W) -> Result<(), io::Error> {
        self.doc.save_to(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_get_names() -> Result<(), LoadError> {
        let form = Form::load("./tests/assets/Formblatt_1.pdf")?;

        let names = form.get_field_names();

        assert!(names.len() > 0);

        Ok(())
    }
}

#[wasm_bindgen]
pub struct JsForm {
    form: Form
}

impl JsForm {
    /// Takes a reader containing a PDF with a fillable form, analyzes the content, and attempts to
    /// identify all of the fields the form has.
    pub fn load_from(form: Form) -> Self {
        JsForm {
            form
        }
    }
}

#[wasm_bindgen]
impl JsForm {
    pub fn get_field_names(&self) -> Box<[JsValue]> {
        let names = self.form.get_field_names();

        let result: Vec<JsValue> = names.iter().map(|x| JsValue::from(x)).collect();

        return result.into_boxed_slice();
    }

    pub fn fill(&mut self, fields: JsValue) -> Result<(), JsValue> {
        let map: HashMap<String, String> = serde_wasm_bindgen::from_value(fields)?;

        self.form.fill(map);

        Ok(())
    }

    pub fn save_to_buf(&mut self) -> Box<[u8]> {
        let mut buffer: Vec<u8> = vec![];
        let mut_buffer: &mut Vec<u8> = buffer.as_mut();

        self.form.save_to(mut_buffer).unwrap();

        return buffer.into_boxed_slice();
    }
}
