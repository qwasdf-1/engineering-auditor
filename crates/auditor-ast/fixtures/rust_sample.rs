// A sample Rust file
fn hello() {
    println!("Hello, world!");
}

fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    fn area(&self) -> i32 {
        self.x * self.y
    }
}
