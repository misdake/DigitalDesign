use crate::DslFunction;
use once_cell::sync::Lazy;

static VEC_NEW: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("vec_new", ["ptr"], []));
static VEC_PUSH: Lazy<DslFunction<2, 0>> =
    Lazy::new(|| DslFunction::new("vec_push", ["ptr", "val"], []));
static VEC_GET: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_get", ["ptr", "index"], ["val"]));
static VEC_REMOVE: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_remove", ["ptr", "index"], ["val"]));
static VEC_POP: Lazy<DslFunction<1, 1>> =
    Lazy::new(|| DslFunction::new("vec_pop", ["ptr"], ["val"]));
static VEC_DROP: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("vec_drop", ["ptr"], []));

//TODO len and cap with mem read directly
