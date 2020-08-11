use lopdf::{Document, Object, ObjectId, StringFormat};

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::str;

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
pub struct Form {
    doc: Document,
    form_ids: Vec<ObjectId>,
}

/// The possible types of fillable form fields in a PDF
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

/// Errors That may occur while setting values in a form
#[derive(Debug, Error)]
pub enum ValueError {
    /// The method used to set the state is incompatible with the type of the field
    TypeMismatch,
    /// One or more selected values are not valid choices
    InvalidSelection,
    /// Multiple values were selected when only one was allowed
    TooManySelected,
}

trait PdfObjectDeref {
    fn deref<'a>(&self, doc: &'a Document) -> Result<&'a Object, LoadError>;
}
