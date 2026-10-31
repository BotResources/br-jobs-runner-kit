use std::any::Any;

pub(crate) fn text_of(payload: Box<dyn Any + Send>) -> String {
    let text = if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with no text".to_owned()
    };
    std::mem::forget(payload);
    text
}
