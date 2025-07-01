mod terminal;

#[tokio::main]
async fn main() {
    terminal::run().expect("Terminal runtime error");
}