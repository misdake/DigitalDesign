//! Two-ended fixed CPU arena. Handles are stable word offsets plus one.
//! Allocation does not initialize payload; producers overwrite defined fields/padding.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;

// Up to fifteen words align the fixed 8-KiB backing without a source-layout promise.
static GPU_OBJECT_BACKING: Buf<u16, 4111> = Buf::new([0; 4111]);
// aligned base, effective words, small/large high water, small/large free heads.
static GPU_OBJECT_CONTROL: Buf<u16, 6> = Buf::new([0; 6]);
#[cfg(gpu_debug)]
static GPU_OBJECT_TAGS: Buf<u16, 256> = Buf::new([0; 256]);

fn gpu_arena_init(bytes: u16) -> bool {
    if bytes == 0 || bytes > GPU_API_ARENA_BYTES || bytes & 31 != 0 {
        return false;
    }
    let mut c = GPU_OBJECT_CONTROL.as_array();
    c[0u16] = (GPU_OBJECT_BACKING.as_ptr().addr() + 15) & 65520;
    c[1u16] = bytes >> 1;
    c[2u16] = 0;
    c[3u16] = 0;
    c[4u16] = 0;
    c[5u16] = 0;
    #[cfg(gpu_debug)]
    {
        let mut n: u16 = 0;
        while n < 256 {
            GPU_OBJECT_TAGS[n] = 0;
            n += 1;
        }
    }
    true
}
fn gpu_arena_data(handle: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(GPU_OBJECT_CONTROL[0u16] + handle - 1).as_u16_array() }
}
fn gpu_arena_alloc(large: bool, kind: u16) -> u16 {
    let mut c = GPU_OBJECT_CONTROL.as_array();
    let mut free_at: u16 = 4;
    let mut words = GPU_API_SMALL_WORDS;
    if large {
        free_at = 5;
        words = GPU_API_LARGE_WORDS;
    }
    let mut handle = c[free_at];
    if handle != 0 {
        c[free_at] = gpu_arena_data(handle)[0u16];
    } else {
        let used = c[2u16] * GPU_API_SMALL_WORDS + c[3u16] * GPU_API_LARGE_WORDS;
        if used > c[1u16] || words > c[1u16] - used {
            return 0;
        }
        if large {
            c[3u16] += 1;
            handle = c[1u16] - c[3u16] * GPU_API_LARGE_WORDS + 1;
        } else {
            handle = c[2u16] * GPU_API_SMALL_WORDS + 1;
            c[2u16] += 1;
        }
    }
    #[cfg(gpu_debug)]
    {
        let mut tag = kind;
        if large {
            tag |= 256;
        }
        GPU_OBJECT_TAGS[(handle - 1) >> 4] = tag;
    }
    handle
}
fn gpu_arena_free(handle: u16, large: bool, kind: u16) -> bool {
    #[cfg(gpu_debug)]
    {
        if handle == 0 || handle - 1 >= GPU_OBJECT_CONTROL[1u16] || (handle - 1) & 15 != 0 {
            return false;
        }
        let index = (handle - 1) >> 4;
        let mut tag = kind;
        if large {
            tag |= 256;
        }
        if GPU_OBJECT_TAGS[index] != tag {
            return false;
        }
        GPU_OBJECT_TAGS[index] = 0;
    }
    let mut free_at: u16 = 4;
    if large {
        free_at = 5;
    }
    gpu_arena_data(handle)[0u16] = GPU_OBJECT_CONTROL[free_at];
    GPU_OBJECT_CONTROL[free_at] = handle;
    true
}
