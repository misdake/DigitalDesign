//! A newly issued synchronous read owns its immutable weights through consumption.
use gpu_v2::texture::emu::unique_weights::{Event, Store};

#[test]
fn public_release_cannot_overlap_a_new_read_of_the_same_owner() {
    let mut store = Store::default();
    store
        .tick(Event {
            publish: Some(0),
            ..Event::default()
        })
        .unwrap();
    store
        .tick(Event {
            write: Some((0, 511)),
            ..Event::default()
        })
        .unwrap();
    let group = (15_u64 << 36) | (3_u64 << 28);
    let result = store.tick(Event {
        read: Some(group),
        free: Some(0),
        ..Event::default()
    });
    assert!(result
        .unwrap_err()
        .contains("before cache weight consumption"));
    assert_eq!((store.paid, store.reads), (None, 0));
    assert!(store
        .tick(Event {
            publish: Some(0),
            ..Event::default()
        })
        .is_err());
    store
        .tick(Event {
            read: Some(group),
            ..Event::default()
        })
        .unwrap();
    assert!(store
        .tick(Event {
            free: Some(0),
            ..Event::default()
        })
        .is_err());
    store.tick(Event::default()).unwrap();
    assert!(store.output().unwrap().is_some());
    store
        .tick(Event {
            pop: true,
            ..Event::default()
        })
        .unwrap();
    store
        .tick(Event {
            free: Some(0),
            ..Event::default()
        })
        .unwrap();
    store
        .tick(Event {
            publish: Some(0),
            ..Event::default()
        })
        .unwrap();
}
