use std::fs;
use std::path::PathBuf;

use boole_testkit::{write_bootable_curl_product_kat_metadata, BootableCurlProductKatInput};

fn main() {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let Some(plan_path) = args.next() else {
        eprintln!("usage: {} PLAN.json", PathBuf::from(program).display());
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("exactly one KAT bundle plan is required");
        std::process::exit(2);
    }
    let result = (|| {
        let plan_raw = fs::read(&plan_path)?;
        let plan: BootableCurlProductKatInput = serde_json::from_slice(&plan_raw)?;
        let roots = write_bootable_curl_product_kat_metadata(plan)?;
        println!("{}", serde_json::to_string(&roots)?);
        Ok::<(), Box<dyn std::error::Error>>(())
    })();
    if let Err(error) = result {
        eprintln!("KAT bundle metadata failed: {error}");
        std::process::exit(1);
    }
}
