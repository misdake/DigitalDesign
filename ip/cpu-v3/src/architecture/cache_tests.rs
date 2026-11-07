use super::*;

fn resident_line(cache: &mut DataCache, address: u32, base: u16, dirty: bool) {
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(address)
        }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: PhysicalWordAddress::new(address),
            }
        ))
    );
    cache
        .complete(MainMemoryResponse::ReadLine { words: line(base) })
        .unwrap();
    if dirty {
        assert_eq!(
            cache.request(CpuMemoryRequest::Write {
                address: PhysicalWordAddress::new(address + 5),
                value: base ^ 0xffff
            }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
        );
    }
}

fn line(base: u16) -> [Word; CACHE_LINE_WORDS] {
    std::array::from_fn(|index| base.wrapping_add(index as u16))
}

#[test]
fn dual_address_copy_discards_target_alias_but_keeps_source_dirty() {
    for destination in [0x240, 0x520, 0x120, 0x3ffff0] {
        let mut cache = DataCache::default();
        resident_line(&mut cache, 0x120, 100, true);
        if destination != 0x120 {
            resident_line(&mut cache, destination, 200, true);
        }
        let source_way = cache
            .store
            .hit_way(PhysicalWordAddress::new(0x120))
            .unwrap();
        let expected = cache
            .store
            .line_words(source_way, decode(PhysicalWordAddress::new(0x120)).set);
        assert_eq!(
            cache
                .copy_line(
                    PhysicalWordAddress::new(0x120),
                    PhysicalWordAddress::new(destination)
                )
                .unwrap(),
            CacheAction::MainMemoryRequest(MainMemoryRequest::WriteLine {
                line_address: PhysicalWordAddress::new(destination),
                words: expected
            })
        );
        cache.complete(MainMemoryResponse::WriteComplete).unwrap();
        assert!(cache
            .store
            .hit_way(PhysicalWordAddress::new(0x120))
            .is_some());
        assert!(cache.dirty[source_way][decode(PhysicalWordAddress::new(0x120)).set]);
        if destination != 0x120 {
            assert!(cache
                .store
                .hit_way(PhysicalWordAddress::new(destination))
                .is_none());
        }
    }
    for (source, destination) in [(0x121, 0x240), (0x120, 0x241)] {
        assert_eq!(
            DataCache::default().copy_line(
                PhysicalWordAddress::new(source),
                PhysicalWordAddress::new(destination)
            ),
            Err(CacheError::UnalignedLine)
        );
    }
}

#[test]
fn dual_address_cold_copy_evicts_source_victim_not_dirty_destination() {
    let mut cache = DataCache::default();
    resident_line(&mut cache, 0x120, 100, true);
    resident_line(&mut cache, 0x520, 200, true);
    resident_line(&mut cache, 0x240, 300, true);
    let action = cache
        .copy_line(
            PhysicalWordAddress::new(0x920),
            PhysicalWordAddress::new(0x240),
        )
        .unwrap();
    assert!(
        matches!(action,CacheAction::MainMemoryRequest(MainMemoryRequest::WriteLine{line_address,..}) if [0x120,0x520].contains(&line_address.get()))
    );
    assert_eq!(
        cache.complete(MainMemoryResponse::WriteComplete).unwrap(),
        CacheAction::MainMemoryRequest(MainMemoryRequest::ReadLine {
            line_address: PhysicalWordAddress::new(0x920)
        })
    );
    let words = line(400);
    assert_eq!(
        cache
            .complete(MainMemoryResponse::ReadLine { words })
            .unwrap(),
        CacheAction::MainMemoryRequest(MainMemoryRequest::WriteLine {
            line_address: PhysicalWordAddress::new(0x240),
            words
        })
    );
    cache.complete(MainMemoryResponse::WriteComplete).unwrap();
    assert!(cache
        .store
        .hit_way(PhysicalWordAddress::new(0x240))
        .is_none());
}

fn read(cache: &mut InstructionCache, address: u32) -> Word {
    match cache
        .request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(address),
        })
        .unwrap()
    {
        CacheAction::CpuResponse(CpuMemoryResponse::Read { value }) => value,
        CacheAction::MainMemoryRequest(MainMemoryRequest::ReadLine { line_address }) => {
            let words = line(line_address.get() as u16);
            match cache
                .complete(MainMemoryResponse::ReadLine { words })
                .unwrap()
            {
                CacheAction::CpuResponse(CpuMemoryResponse::Read { value }) => value,
                _ => panic!("expected a read response"),
            }
        }
        _ => panic!("instruction cache produced an unexpected action"),
    }
}

#[test]
fn geometry_is_two_ways_of_sixty_four_sixteen_word_lines() {
    assert_eq!(CACHE_CAPACITY_BYTES, 4_096);
    assert_eq!(CACHE_WAYS * CACHE_SETS * CACHE_LINE_WORDS, 2_048);
    assert_eq!(CACHE_WAYS * CACHE_SETS * (CACHE_LINE_WORDS / 2), 1_024);
}

#[test]
fn instruction_cache_refills_then_hits_within_the_line() {
    let mut cache = InstructionCache::default();
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(0x1237)
        }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: PhysicalWordAddress::new(0x1230)
            }
        ))
    );
    assert_eq!(
        cache.request(CpuMemoryRequest::Read { address: 0.into() }),
        Err(CacheError::Busy)
    );
    assert_eq!(
        cache.complete(MainMemoryResponse::ReadLine { words: line(100) }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
            value: 107
        }))
    );
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(0x123f)
        }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
            value: 115
        }))
    );
}

#[test]
fn instruction_cache_invalidates_every_resident_line() {
    let mut cache = InstructionCache::default();
    assert_eq!(read(&mut cache, 0x2012), 0x2012);
    cache.invalidate_all().unwrap();
    assert!(matches!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(0x2012)
        }),
        Ok(CacheAction::MainMemoryRequest(_))
    ));
}

#[test]
fn data_cache_write_hit_stays_in_the_cache_without_memory_traffic() {
    let mut cache = DataCache::default();
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(0x1230)
        }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: PhysicalWordAddress::new(0x1230)
            }
        ))
    );
    cache
        .complete(MainMemoryResponse::ReadLine { words: line(100) })
        .unwrap();
    // A write hit updates the resident word, marks it dirty, and issues no
    // memory request.
    assert_eq!(
        cache.request(CpuMemoryRequest::Write {
            address: PhysicalWordAddress::new(0x1234),
            value: 0xabcd
        }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
    );
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(0x1234)
        }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
            value: 0xabcd
        }))
    );
}

#[test]
fn clean_line_writes_one_dirty_hit_and_keeps_it_resident() {
    let mut cache = DataCache::default();
    let address = PhysicalWordAddress::new(0x1234);
    assert!(matches!(
        cache.request(CpuMemoryRequest::Read { address }).unwrap(),
        CacheAction::MainMemoryRequest(MainMemoryRequest::ReadLine { .. })
    ));
    cache
        .complete(MainMemoryResponse::ReadLine { words: line(100) })
        .unwrap();
    cache
        .request(CpuMemoryRequest::Write {
            address,
            value: 0xabcd,
        })
        .unwrap();
    assert!(matches!(
        cache.clean_line(address).unwrap(),
        CacheAction::MainMemoryRequest(MainMemoryRequest::WriteLine {
            line_address,
            words
        }) if line_address == PhysicalWordAddress::new(0x1230) && words[4] == 0xabcd
    ));
    assert_eq!(
        cache.complete(MainMemoryResponse::WriteComplete),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
    );
    assert_eq!(cache.dirty_bits(), 0);
    assert_eq!(
        cache.request(CpuMemoryRequest::Read { address }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
            value: 0xabcd
        }))
    );
}

#[test]
fn data_cache_write_miss_read_allocates_the_line() {
    let mut cache = DataCache::default();
    assert_eq!(
        cache.request(CpuMemoryRequest::Write {
            address: PhysicalWordAddress::new(0x1234),
            value: 0xabcd
        }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: PhysicalWordAddress::new(0x1230)
            }
        ))
    );
    assert_eq!(
        cache.complete(MainMemoryResponse::ReadLine { words: line(100) }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
    );
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(0x1234)
        }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
            value: 0xabcd
        }))
    );
}

#[test]
fn line_copy_refills_source_redirects_writeback_and_preserves_source_dirty() {
    let mut cache = DataCache::default();
    let source = PhysicalWordAddress::new(0x1230);
    assert_eq!(
        cache.copy_line(
            source,
            PhysicalWordAddress::new((3 << 14) | (source.get() & 0x3fff))
        ),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: source
            }
        ))
    );
    let source_words = line(0x4000);
    assert_eq!(
        cache.complete(MainMemoryResponse::ReadLine {
            words: source_words
        }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::WriteLine {
                line_address: PhysicalWordAddress::new(0x0000_d230),
                words: source_words
            }
        ))
    );
    assert_eq!(
        cache.complete(MainMemoryResponse::WriteComplete),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
    );

    cache
        .request(CpuMemoryRequest::Write {
            address: PhysicalWordAddress::new(0x1232),
            value: 0x5a5a,
        })
        .unwrap();
    let mut dirty_words = source_words;
    dirty_words[2] = 0x5a5a;
    assert_eq!(
        cache.copy_line(
            source,
            PhysicalWordAddress::new((4 << 14) | (source.get() & 0x3fff))
        ),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::WriteLine {
                line_address: PhysicalWordAddress::new(0x0001_1230),
                words: dirty_words
            }
        ))
    );
    cache.complete(MainMemoryResponse::WriteComplete).unwrap();
    assert_eq!(
        cache.begin_maintenance(MaintenanceCommand::Clean),
        Ok(Some(MainMemoryRequest::WriteLine {
            line_address: source,
            words: dirty_words
        }))
    );
}

#[test]
fn cold_copy_reuses_invalidated_destination_way_and_preserves_other_resident() {
    let mut cache = DataCache::default();
    for (address, base) in [(0x20, 0xa000), (0x4020, 0xb000)] {
        assert!(matches!(
            cache
                .request(CpuMemoryRequest::Read {
                    address: PhysicalWordAddress::new(address),
                })
                .unwrap(),
            CacheAction::MainMemoryRequest(MainMemoryRequest::ReadLine { .. })
        ));
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(base) })
            .unwrap();
    }
    cache
        .request(CpuMemoryRequest::Write {
            address: PhysicalWordAddress::new(0x4020),
            value: 0xdead,
        })
        .unwrap();
    // The destination lives in way one, while replacement would ordinarily
    // choose way zero. Overwriting the destination frees way one first.
    assert_eq!(
        cache
            .copy_line(
                PhysicalWordAddress::new(0x8020),
                PhysicalWordAddress::new(0x4020)
            )
            .unwrap(),
        CacheAction::MainMemoryRequest(MainMemoryRequest::ReadLine {
            line_address: PhysicalWordAddress::new(0x8020),
        })
    );
    assert_eq!(
        cache
            .complete(MainMemoryResponse::ReadLine {
                words: line(0xc000)
            })
            .unwrap(),
        CacheAction::MainMemoryRequest(MainMemoryRequest::WriteLine {
            line_address: PhysicalWordAddress::new(0x4020),
            words: line(0xc000),
        })
    );
    cache.complete(MainMemoryResponse::WriteComplete).unwrap();
    assert_eq!(
        cache
            .request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(0x20)
            })
            .unwrap(),
        CacheAction::CpuResponse(CpuMemoryResponse::Read { value: 0xa000 })
    );
}

#[test]
fn dirty_victim_is_written_back_before_the_incoming_line_installs() {
    let stride = (CACHE_SETS * CACHE_LINE_WORDS) as u32;
    let mut cache = DataCache::default();

    // Line 0 in way 0, made dirty by a store.
    assert_eq!(
        cache.request(CpuMemoryRequest::Read { address: 0.into() }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: PhysicalWordAddress::new(0)
            }
        ))
    );
    cache
        .complete(MainMemoryResponse::ReadLine { words: line(10) })
        .unwrap();
    cache
        .request(CpuMemoryRequest::Write {
            address: 0.into(),
            value: 0xbeef,
        })
        .unwrap();

    // Line 64 in way 1 (same set, distinct tag).
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(stride)
        }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: PhysicalWordAddress::new(stride)
            }
        ))
    );
    cache
        .complete(MainMemoryResponse::ReadLine { words: line(20) })
        .unwrap();

    // Line 128 (same set) evicts the dirty way 0: it writes line 0 back
    // first (with the stored word), then requests line 128.
    let mut victim = line(10);
    victim[0] = 0xbeef;
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(2 * stride)
        }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::WriteLine {
                line_address: PhysicalWordAddress::new(0),
                words: victim,
            }
        ))
    );
    assert_eq!(
        cache.complete(MainMemoryResponse::WriteComplete),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: PhysicalWordAddress::new(2 * stride)
            }
        ))
    );
    cache
        .complete(MainMemoryResponse::ReadLine { words: line(30) })
        .unwrap();

    // Line 64 is intact, and the evicted line 0 now misses again.
    assert_eq!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(stride)
        }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
            value: 20
        }))
    );
    assert!(matches!(
        cache.request(CpuMemoryRequest::Read { address: 0.into() }),
        Ok(CacheAction::MainMemoryRequest(_))
    ));
}

fn dirty_line(cache: &mut DataCache, address: u32) {
    assert!(matches!(
        cache.request(CpuMemoryRequest::Read {
            address: PhysicalWordAddress::new(address)
        }),
        Ok(CacheAction::MainMemoryRequest(_))
    ));
    cache
        .complete(MainMemoryResponse::ReadLine {
            words: line((address & 0xff) as u16),
        })
        .unwrap();
    cache
        .request(CpuMemoryRequest::Write {
            address: PhysicalWordAddress::new(address),
            value: 0xdead,
        })
        .unwrap();
}

#[test]
fn clean_writes_back_each_dirty_line_exactly_once_and_keeps_lines_valid() {
    let stride = (CACHE_SETS * CACHE_LINE_WORDS) as u32;
    let mut cache = DataCache::default();
    dirty_line(&mut cache, 0);
    dirty_line(&mut cache, stride);

    let mut written = 0;
    let mut request = cache.begin_maintenance(MaintenanceCommand::Clean).unwrap();
    while let Some(req) = request {
        match req {
            MainMemoryRequest::WriteLine { .. } => written += 1,
            _ => panic!("clean must only issue write-backs"),
        }
        request = cache
            .continue_maintenance(MainMemoryResponse::WriteComplete)
            .unwrap();
    }
    assert_eq!(written, 2);

    // Both lines remain valid (clean), so a second clean writes nothing.
    assert_eq!(
        cache.begin_maintenance(MaintenanceCommand::Clean).unwrap(),
        None
    );
    assert!(matches!(
        cache.request(CpuMemoryRequest::Read { address: 0.into() }),
        Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read { .. }))
    ));
}

#[test]
fn invalidate_writes_back_dirty_lines_then_clears_every_way() {
    let mut cache = DataCache::default();
    dirty_line(&mut cache, 0);

    assert!(matches!(
        cache
            .begin_maintenance(MaintenanceCommand::Invalidate)
            .unwrap(),
        Some(MainMemoryRequest::WriteLine { .. })
    ));
    assert_eq!(
        cache
            .continue_maintenance(MainMemoryResponse::WriteComplete)
            .unwrap(),
        None
    );

    // Every line is now invalid, so the read misses again.
    assert!(matches!(
        cache.request(CpuMemoryRequest::Read { address: 0.into() }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine { .. }
        ))
    ));
}

#[test]
fn equal_offsets_in_different_segments_have_distinct_tags() {
    let mut cache = InstructionCache::default();
    let first = PhysicalWordAddress::from_segment_offset(1, 0x1234);
    let second = PhysicalWordAddress::from_segment_offset(2, 0x1234);
    cache
        .request(CpuMemoryRequest::Read { address: first })
        .unwrap();
    cache
        .complete(MainMemoryResponse::ReadLine { words: line(10) })
        .unwrap();
    assert_eq!(
        cache.request(CpuMemoryRequest::Read { address: second }),
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: second.line_base(CACHE_LINE_WORDS as u32),
            }
        ))
    );
}
