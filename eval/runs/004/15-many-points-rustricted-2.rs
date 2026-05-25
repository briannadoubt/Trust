#![strict]

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
    let origin = make_point(x: 0, y: 0, z: 0);
    let a = make_point(x: 3, y: 4, z: 0);
    let b = make_point(x: 0, y: 0, z: 5);
    let c = make_point(x: 1, y: 1, z: 1);

    println!("{:.4}", distance(origin, a));
    println!("{:.4}", distance(origin, b));
    println!("{:.4}", distance(origin, c));
}
