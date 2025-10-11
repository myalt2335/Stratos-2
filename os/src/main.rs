fn main() {
    println!("{}", std::env::var("UEFI_IMG").unwrap());
}
