extern crate lopdf;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate derive_error;
extern crate web_sys;


mod pdfformfill;
mod utils;

use wasm_bindgen::prelude::*;
use crate::pdfformfill::Form;
use wasm_bindgen::__rt::std::io::{BufReader};
use crate::utils::set_panic_hook;
use std::collections::HashMap;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

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

        self.form.fill(map).map_err(|x| serde_wasm_bindgen::to_value(&x).unwrap())?;

        Ok(())
    }

    pub fn save_to_buf(&mut self) -> Box<[u8]> {
        let mut buffer: Vec<u8> = vec![];
        let mut_buffer: &mut Vec<u8> = buffer.as_mut();

        self.form.save_to(mut_buffer).unwrap();

        return buffer.into_boxed_slice();
    }
}


#[wasm_bindgen]
pub fn load_form(bytes: &[u8]) -> JsForm {
    set_panic_hook();

    let reader = BufReader::new(bytes);

    let form = Form::load_from(reader);

    return JsForm::load_from(form.unwrap());
}
