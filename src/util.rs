use std::env;

pub fn should_unset_write_callback() -> bool {
    let all_args = env::args().collect::<Vec<String>>();
    all_args.contains(&"segfault".into())
}
