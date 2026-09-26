#[cfg(target_os = "linux")]
#[test]
fn glib_variant_string_iteration() {
    use glib::prelude::*;
    let values = ["zero", "one", "two", "three", "four", "five"];
    for _ in 0..1000 {
        let variant = values.to_variant();
        assert_eq!(variant.array_iter_str().unwrap().collect::<Vec<_>>(), values);
        let mut iter = variant.array_iter_str().unwrap();
        assert_eq!(iter.nth(1), Some("one"));
        assert_eq!(iter.next(), Some("two"));
        assert_eq!(iter.nth_back(1), Some("four"));
        assert_eq!(iter.next_back(), Some("three"));
        assert_eq!(iter.next(), None);
        assert_eq!(variant.array_iter_str().unwrap().last(), Some("five"));
    }
}
