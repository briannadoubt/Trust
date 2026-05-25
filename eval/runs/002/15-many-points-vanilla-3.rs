fn make_point(x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    (x, y, z)
}

fn distance(p1: (i32, i32, i32), p2: (i32, i32, i32)) -> f64 {
    let dx = (p1.0 - p2.0) as f64;
    let dy = (p1.1 - p2.1) as f64;
    let dz = (p1.2 - p2.2) as f64;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn main() {
    let origin = make_point(0, 0, 0);
    let a = make_point(3, 4, 0);
    let b = make_point(0, 0, 5);
    let c = make_point(1, 1, 1);

    println!("{:.4}", distance(origin, a));
    println!("{:.4}", distance(origin, b));
    println!("{:.4}", distance(origin, c));
}
