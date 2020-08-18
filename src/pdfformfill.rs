use std::{io, str};
use std::collections::VecDeque;
use std::path::Path;

use lopdf::{Dictionary, Document, Error, Object, ObjectId, StringFormat};
use regex::Regex;
use serde::Serialize;
use wasm_bindgen::__rt::std::collections::HashMap;
use wasm_bindgen::prelude::*;

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

trait ToPdfUTF16 {
    fn to_pdf_utf16(&self) -> Vec<u8>;
}

impl ToPdfUTF16 for String {
    fn to_pdf_utf16(&self) -> Vec<u8> {
        let bytes: Vec<u16> = self.encode_utf16().collect();
        let mut res: Vec<u8> = vec![0; bytes.len() * 2 + 2];

        // first bits indicate the string is UTF16 encoded
        res[0] = 0xfe;
        res[1] = 0xff;

        for (i, byte) in bytes.iter().enumerate() {
            res[2 * i + 2] = (*byte >> 8) as u8;
            res[2 * i + 3] = (*byte & 0xff) as u8;
        }

        res
    }
}

/// Errors That may occur while setting values in a form
#[wasm_bindgen]
#[derive(Serialize, Debug, Error)]
pub enum ValueError {
    /// The method used to set the state is incompatible with the type of the field
    TypeMismatch,
    /// One or more selected values are not valid choices
    InvalidSelection,
    /// Multiple values were selected when only one was allowed
    TooManySelected,
}

/// Error that may occur while setting a value on a specific field
#[wasm_bindgen]
#[derive(Serialize, Debug)]
pub struct FieldError {
    error: ValueError,
    field: String,
    value: String,
}

#[wasm_bindgen]
impl FieldError {
    pub fn new(error: ValueError, field: String, value: String) -> Self {
        FieldError { field, error, value }
    }
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

                        if let Some(ref name) = Form::get_full_name(&doc, &field_id) {
                            // let name = Form::get_form_name(string_u8.clone())?;
                            map.insert(name.clone(), field_id);
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

    fn get_full_name(doc: &Document, field_id: &ObjectId) -> Option<String> {
        let field = doc.objects.get(field_id)?;
        let field_dict = field.as_dict().ok()?;

        let field_name = Form::get_field_name(doc, field_id)?;



        if let Ok(Object::Reference(ref parent_ref)) = field_dict.get(b"Parent") {
            let parent_name = Form::get_full_name(doc, parent_ref)?;
            return Some(format!("{}.{}", parent_name, field_name));
        } else {
            return Some(field_name);
        }
    }

    fn get_field_name(doc: &Document, field_id: &ObjectId) -> Option<String> {
        let field = doc.objects.get(field_id).unwrap();
        let field_dict = field.as_dict().unwrap();
        if let Ok(Object::String(ref f_string_u8, _)) = field_dict.get(b"T") {
            if let Some(ref field_name) = Form::encode_form_name(f_string_u8.clone()) {
                return Some(field_name.clone());
            }
        }

        None
    }

    fn encode_form_name(string_u8: Vec<u8>) -> Option<String> {
        // Assuming the string is UTF16. First 2 Bytes indicate UTF16, so we skip them
        // Converting 8bit array to 16bit array
        let mut string_u16 = vec![0; string_u8.len()];
        for (i, byte) in string_u8.iter().skip(2).enumerate() {
            string_u16[i / 2] = if i % 2 == 0 { (u16::from(*byte)) << 8 } else { u16::from(*byte) };
        }

        // The first \0 indicates the end of a string
        let str_end = string_u16.iter().position(|x| *x == 0 as u16).unwrap_or(string_u16.len());

        if let Ok(ref name) = String::from_utf16(&string_u16[..str_end]) {
            return Some(name.into());
        }

        None
    }

    /// Returns the number of fields the form has
    pub fn len(&self) -> usize {
        self.form_fields.len()
    }

    /// Gets the type of field of the given index
    ///
    /// # Panics
    /// This function will panic if the index is greater than the number of fields
    pub fn get_type(&self, name: &String) -> Result<FieldType, LoadError> {
        let field_id = self.form_fields.get(name.as_str()).ok_or(LoadError::DictionaryKeyNotFound)?;
        let field = self.doc.objects.get(&field_id).unwrap().as_dict().unwrap();
        let obj_zero = Object::Integer(0);
        let type_str = field.get(b"FT").unwrap().as_name_str().unwrap();
        if type_str == "Btn" {
            let flags = ButtonFlags::from_bits_truncate(field.get(b"Ff").unwrap_or(&obj_zero).as_i64().unwrap() as u32);
            if flags.intersects(ButtonFlags::RADIO) {
                Ok(FieldType::Radio)
            } else if flags.intersects(ButtonFlags::PUSHBUTTON) {
                Ok(FieldType::Button)
            } else {
                Ok(FieldType::CheckBox)
            }
        } else if type_str == "Ch" {
            let flags = ChoiceFlags::from_bits_truncate(field.get(b"Ff").unwrap_or(&obj_zero).as_i64().unwrap() as u32);
            if flags.intersects(ChoiceFlags::COBMO) {
                Ok(FieldType::ComboBox)
            } else {
                Ok(FieldType::ListBox)
            }
        } else {
            Ok(FieldType::Text)
        }
    }

    /// Gets the types of all of the fields in the form
    pub fn get_all_types(&self) -> Vec<FieldType> {
        self.form_fields.keys().cloned().map(|f| self.get_type(&f).unwrap()).collect::<Vec<FieldType>>()
    }

    /// Gets the state of field of the given index
    ///
    /// # Panics
    /// This function will panic if the index is greater than the number of fields
    pub fn get_state(&self, name: &String) -> FieldState {
        let field_id = self.form_fields.get(name).unwrap();
        let field = self.doc.objects.get(&field_id).unwrap().as_dict().unwrap();
        match self.get_type(name).unwrap() {
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
    pub fn set_text(&mut self, name: &String, s: String) -> Result<(), ValueError> {
        match self.get_type(name) {
            Ok(FieldType::Text) => {
                let field = self.doc.objects.get_mut(&self.form_fields[name]).unwrap().as_dict_mut().unwrap();

                field.set("V", Object::String(s.to_pdf_utf16(), StringFormat::Literal));
                field.remove(b"AP");
                Ok(())
            }
            _ => Err(ValueError::TypeMismatch)
        }
    }

    /// If the field at index `n` is a radio field, toggles the radio button based on the value
    /// `choice`
    /// If it is not a radio button field or the choice is not a valid option, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_radio(&mut self, name: &String, choice: String) -> Result<(), ValueError> {
        let field_id = self.form_fields.get(name).unwrap();

        match self.get_state(name) {
            FieldState::Radio { selected: _, options } => if options.contains(&choice) {
                let mut doc_objects = self.doc.objects.clone();
                let field = doc_objects.get_mut(field_id).unwrap().as_dict_mut().unwrap();

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
                Err(ValueError::InvalidSelection)
            },
            _ => Err(ValueError::TypeMismatch)
        }
    }


    /// If the field at index `n` is a checkbox field, toggles the check box based on the value
    /// `is_checked`.
    /// If it is not a checkbox field, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_check_box(&mut self, name: &String, is_checked: bool) -> Result<(), ValueError> {
        match self.get_type(name) {
            Ok(FieldType::CheckBox) => {
                let state = Object::Name({ if is_checked { "Yes" } else { "Off" } }.to_owned().into_bytes());
                let field = self.doc.objects.get_mut(&self.form_fields.get(name).unwrap()).unwrap().as_dict_mut().unwrap();
                field.set("V", state.clone());
                field.set("AS", state);
                Ok(())
            }
            _ => Err(ValueError::TypeMismatch)
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
    pub fn fill(&mut self, fields: &HashMap<String, String>) -> Result<(), FieldError> {
        let r = Regex::new(r"\[\d+]").unwrap();

        for field_name in self.form_fields.clone().keys() {
            let mut part_names: Vec<_> = field_name.split(".").collect();

            let mut name: String = field_name.clone();
            let mut i = 0;
            while part_names.len() >= 1 && !fields.contains_key(&name) {
                if i == 0 {
                    i = 1;
                    name = r.replace(name.as_str(), "").into()
                } else {
                    i = 0;
                    part_names = part_names[1..].to_vec();
                    name = part_names.join(".").into();
                }
            }

            // The field was not provided
            if part_names.is_empty() {
                continue;
            }

            let map_v = fields.get(&name).unwrap();
            let map_err = |x: ValueError| FieldError::new(x, name.clone(), map_v.clone());

            match self.get_type(&field_name) {
                Ok(FieldType::Radio) => {
                    self.set_radio(&field_name, map_v.clone()).map_err(map_err)?;
                }
                Ok(FieldType::CheckBox) => {
                    self.set_check_box(&field_name, map_v.clone().to_lowercase().eq("true")).map_err(map_err)?;
                }
                Ok(FieldType::Text) => {
                    self.set_text(&field_name, map_v.clone()).map_err(map_err)?;
                }
                _ => {}
            };

        }

        Ok(())
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

    #[test]
    pub fn test_write_utf8() -> Result<(), LoadError> {
        let mut form = Form::load("./tests/assets/Formblatt_1.modified.pdf")?;

        let mut map: HashMap<String, String> = HashMap::new();
        map.insert(String::from("Name_Eingabe"), String::from("Björn"));
        form.fill(&map);

        form.save("./Formblatt_1.pdf")?;


        assert!(true);

        Ok(())
    }
}