fn double(x: i32) -> i32 { x * 2 }
fn add_one(x: i32) -> i32 { x + 1 }

fn main() {
    let result = 5 |> double() |> add_one();
    println!("{result}");
}
