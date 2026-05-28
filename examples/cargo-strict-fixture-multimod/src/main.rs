trust_attrs::strict! {}

mod geom;

fn main() {
    let r = geom::make_rect(width: 10, height: 5);
    println!("{}", geom::area(r));
}
