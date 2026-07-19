fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: stmt_hash_cli <export.ndjson> <full_decl_name>");
        std::process::exit(2);
    }
    let bytes = std::fs::read(&args[1]).expect("read");
    match stmt_hash::statement_hash(&bytes, &args[2]) {
        Ok(Some(h)) => println!("{h}"),
        Ok(None) => {
            eprintln!("declaration not found");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("parse error: {e}");
            std::process::exit(1);
        }
    }
}
