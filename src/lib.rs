extern crate lopdf;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate derive_error;
extern crate web_sys;


mod pdfformfill;
mod utils;

use wasm_bindgen::prelude::*;
use crate::pdfformfill::{Form, FieldType};
use wasm_bindgen::__rt::std::io::{BufReader};
use serde::Serializer;
use crate::utils::set_panic_hook;
use serde_wasm_bindgen::Error;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern {
    fn alert(s: &str);
}

impl serde::ser::Serialize for pdfformfill::FieldType {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error> where
        S: Serializer {
        serializer.collect_str(match self {
            FieldType::Radio => "Radio",
            FieldType::Button => "Button",
            FieldType::CheckBox => "CheckBox",
            FieldType::ListBox => "ListBox",
            FieldType::ComboBox => "ComboBox",
            FieldType::Text => "Text",
        })
    }
}

#[wasm_bindgen]
pub fn greet() {
    set_panic_hook();

    web_sys:: console::log_1(&"Hello, world!".into());
    alert("Hello, pdfformfill!");
}

#[wasm_bindgen]
pub fn get_field_types(p: JsValue) -> JsValue {
    set_panic_hook();

    web_sys::console::log_1(&p);

    let bytes: Vec<u8> = serde_wasm_bindgen::from_value(p).unwrap();
    let r = BufReader::new(&bytes[..]);
    let result = Form::load_from(r);

    let form = result.unwrap();

    let fields = &form.get_all_types()[..];

    return serde_wasm_bindgen::to_value(&fields).unwrap();
}

#[wasm_bindgen]
pub fn load_form(bytes: &[u8]) -> Form {
    set_panic_hook();

    let reader = BufReader::new(bytes);

    let form = Form::load_from(reader);

    return form.unwrap();
}
