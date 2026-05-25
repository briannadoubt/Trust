fn rect_area(width: u32, height: u32) -> u32 { width * height }

fn main() {
    let area = rect_area(1920, 1080);
    println!("area = {}", area);
}
