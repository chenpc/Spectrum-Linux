fn main() {
    let d = gdk4::Display::default();
    println!("default: {:?}", d);
    let o = gdk4::Display::open(None);
    println!("open: {:?}", o);
}
