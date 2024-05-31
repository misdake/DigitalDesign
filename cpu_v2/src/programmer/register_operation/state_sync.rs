use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Move<T: Debug + Copy + Eq + PartialEq + Hash + Ord + PartialOrd>(pub T, pub T);

pub fn move_items<T: Debug + Copy + Eq + PartialEq + Hash + Ord + PartialOrd>(
    mapping: BTreeMap<T, T>,
    tmp: T,
) -> Vec<Move<T>> {
    let mut r = vec![];

    let mut done = HashSet::<T>::new();
    let mut stack = vec![];

    for from in mapping.keys().cloned() {
        if done.contains(&from) {
            continue;
        }
        let mut from = from;
        let mut last_move = None;

        stack.push(from);
        while let Some(to) = mapping.get(&from).cloned() {
            // found loop
            if stack.contains(&to) {
                if from != to {
                    r.push(Move(from, tmp));
                    last_move = Some(Move(tmp, to));
                }
                break;
            }
            stack.push(to);
            from = to;
        }

        let mut to = stack.pop().unwrap();
        done.insert(to);
        while let Some(from) = stack.pop() {
            done.insert(from);
            r.push(Move(from, to));
            to = from;
        }
        if let Some(last) = last_move {
            r.push(last);
        }
    }

    r
}

#[test]
fn test_move_items() {
    let moves = move_items(BTreeMap::from([(0, 0), (1, 2), (3, 4), (2, 3)]), 10);
    for m in &moves {
        println!("{m:?}");
    }
    assert_eq!(moves, vec![Move(3, 4), Move(2, 3), Move(1, 2)]);
    println!("-----------");
    let moves = move_items(
        BTreeMap::from([(0, 1), (1, 2), (2, 0), (3, 4), (4, 3), (5, 5)]),
        10,
    );
    for m in &moves {
        println!("{m:?}");
    }
    assert_eq!(
        moves,
        vec![
            Move(2, 10),
            Move(1, 2),
            Move(0, 1),
            Move(10, 0),
            Move(4, 10),
            Move(3, 4),
            Move(10, 3)
        ]
    );
    println!("-----------");
}
