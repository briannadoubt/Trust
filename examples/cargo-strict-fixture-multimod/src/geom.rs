rustricted_attrs::strict! {}

pub fn make_rect(width: u32, height: u32) -> (u32, u32) {
    (width, height)
}

pub fn area(rect: (u32, u32)) -> u32 {
    rect.0 * rect.1
}
