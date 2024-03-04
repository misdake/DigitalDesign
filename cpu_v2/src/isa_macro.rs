use cpu_macro::define_isa;

define_isa! {
    (mov 0b0001 ORRR "r{0} = r{1}")
    (add 0b0001 ORRR "r{0} = r{1} + r{2}")
}
