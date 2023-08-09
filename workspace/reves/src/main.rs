use clap::Parser;

fn main() {
    let args: reves::Args = reves::Args::parse();
    reves::lib_main(&args);
}
