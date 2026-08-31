/// The engine's display name. Every core function must carry a unit test.
fn engine_name() -> &'static str {
    "XEngine"
}

fn main() {
    println!("Hello, world! ({})", engine_name());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_name_is_consistent() {
        assert_eq!(engine_name(), "XEngine");
    }
}
