//! Deterministic immutable native assets and an RCC-recorded drawing program.
use std::collections::BTreeMap;
pub const ASSET: u32 = 0x4b0000;
pub const TEXTURE: u32 = ASSET + 0x1000;
pub fn expected_commands(object_word: u32) -> Vec<u8> {
    use gpu_v2::system::vliw_command::{Command as C, CommandBuffer};
    let mut cb = CommandBuffer::new();
    let mut cursor = 0x4a0000 + object_word * 2;
    let alloc = |cursor: &mut u32, n: u32| {
        *cursor = cursor.next_multiple_of(n);
        let a = *cursor;
        *cursor += n;
        a
    };
    let target = alloc(&mut cursor, 32);
    for c in [
        C::wait(true, true),
        C::FLUSH,
        C::dma_local(target, 276, 32).unwrap(),
        C::wait(true, false),
        C::set_target(276).unwrap(),
        C::clear(false, 2),
        C::clear(true, 65535),
    ] {
        cb.push(c);
    }
    let state = |cb: &mut CommandBuffer, cursor: &mut u32, global: bool| {
        let n = if global { 64 } else { 32 };
        let q = if global { 264 } else { 272 };
        let source = alloc(cursor, n);
        for c in [
            C::wait(true, true),
            C::dma_local(source, q, n).unwrap(),
            C::wait(true, false),
            if global {
                C::set_global(q).unwrap()
            } else {
                C::set_render(q).unwrap()
            },
        ] {
            cb.push(c);
        }
    };
    let draw = |cb: &mut CommandBuffer, cursor: &mut u32, records: &[u32]| {
        let instance = alloc(cursor, 128);
        cb.push(C::dma_local(instance, 320, 128).unwrap());
        cb.push(C::wait(true, false));
        cb.push(C::dma_meshlet(records[0], 0, 128).unwrap());
        cb.push(C::wait(true, false));
        for i in 0..records.len() {
            if i + 1 < records.len() {
                cb.push(C::dma_meshlet(records[i + 1], ((i + 1) & 1) as u8, 128).unwrap());
            }
            cb.push(C::draw_meshlet((i & 1) as u8).unwrap());
            if i + 1 < records.len() {
                cb.push(C::wait(true, false));
            }
        }
    };
    state(&mut cb, &mut cursor, true);
    state(&mut cb, &mut cursor, false);
    draw(
        &mut cb,
        &mut cursor,
        &[ASSET + 128, ASSET + 384, ASSET + 640],
    );
    draw(
        &mut cb,
        &mut cursor,
        &[ASSET + 128, ASSET + 384, ASSET + 640],
    );
    state(&mut cb, &mut cursor, true);
    state(&mut cb, &mut cursor, false);
    draw(&mut cb, &mut cursor, &[ASSET + 896, ASSET + 1152]);
    state(&mut cb, &mut cursor, false);
    draw(&mut cb, &mut cursor, &[ASSET + 128]);
    for c in [C::wait(true, true), C::FLUSH, C::END] {
        cb.push(c);
    }
    cb.image().unwrap().storage
}
pub fn program() -> String {
    r#"
mod gpu;
mod gpu_transform;
static MATRIX:Buf<u16,42>=Buf::new([0;42]);
static STEP:u16=0;
fn check(ok:bool){if !ok{halt(0xb00+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){
 unsafe {write_dseg_page(3,19);}
 check(gpu_init()==GPU_IDLE);
 let mut mesh:GpuMesh=GpuMesh{directory_sector:0x9600,directory_word:0,count:3,first_sector:0x9600,sectors:32};
 let mut uv:GpuMesh=GpuMesh{directory_sector:0x9600,directory_word:6,count:2,first_sector:0x9600,sectors:32};
 let mut empty:GpuMesh=GpuMesh{directory_sector:0x9600,directory_word:0,count:0,first_sector:0x9600,sectors:32};
 let mut texture:GpuTexture=GpuTexture{sector:0x9620,sectors:1,size_log2:0,full_mip:0};
 let mut g:GpuGlobal=GpuGlobal{far_low:0,far_high:100,light_x:0,light_y:0,light_z:16384,ray_x:0,ray_y:0,ray_k:8192,ambient:256,directional:0,mode:0,shininess:0,epoch:0};
 let mut r:GpuRender=GpuRender{texture_sector:0,texture_sectors:0,size_log2:0,full_mip:0,filter:0,specular_low:0,specular_high:0,depth_func:7,depth_write:1,blend:0,unlit:1,lod_bias:0};
 let mut iteration:u16=0;
 while iteration<2 {
   let cb=gpu_cb_begin();check(cb==GPU_CB_HANDLE);
   // Diagnostic placement exercises the second DSEG object page; the app API remains opaque.
   if iteration==1 {GPU_CB.object_words=16368;}
   let pass=gpu_pass_begin(cb,iteration+1,65535);check(pass==GPU_CB_HANDLE);
   check(gpu_pass_begin(cb,0,0)==GPU_CB_NONE);
   check(gpu_pass_draw_mesh(pass,view_of(&mut empty),MATRIX.as_array(),65535,65535));
   check(!gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),65535,65535));
   r.unlit=1;r.blend=0;r.depth_write=1;
   check(gpu_pass_set_global(pass,view_of(&mut g)));
   check(gpu_pass_set_global(pass,view_of(&mut g)));
   check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
   check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
   gpu_affine_identity(MATRIX.as_array());
   gpu_fstore(MATRIX.as_array(),3,fix32::from_words(32768,65535));
   gpu_fstore(MATRIX.as_array(),7,fix32::from_words(57344,65535));
   gpu_fstore(MATRIX.as_array(),12,fix32::from_int(1));
   gpu_fstore(MATRIX.as_array(),16,fix32::from_int(1));
   gpu_fstore(MATRIX.as_array(),20,fix32::from_int(1));
   check(gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),65535,65535));
   // Reuse the source object, but the first Instance must remain a snapshot.
   gpu_fstore(MATRIX.as_array(),3,fix32::zero());
   check(gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),65408,65535));
   r.unlit=0;g.epoch=iteration+1;
   check(gpu_pass_set_global(pass,view_of(&mut g)));
   check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
   gpu_fstore(MATRIX.as_array(),7,fix32::from_words(8192,0));
   check(gpu_pass_draw_mesh(pass,view_of(&mut uv),MATRIX.as_array(),65535,65535));
   // Transparent overlay: last draw stays last, and must see the old color.
   r.unlit=1;r.blend=1;r.depth_write=0;
   check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
   mesh.count=1;
   gpu_fstore(MATRIX.as_array(),3,fix32::from_words(32768,65535));
   gpu_fstore(MATRIX.as_array(),7,fix32::from_words(57344,65535));
   check(gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),0,32768));
   mesh.count=3;
   check(gpu_pass_end(pass));check(!gpu_pass_end(pass));
   check(gpu_cb_finish(cb));check(!gpu_cb_finish(cb));
   check(!gpu_pass_set_global(pass,view_of(&mut g)));
   check(read_dseg_page(3)==19);
   check(gpu_submit_idle(cb)==GPU_PENDING);check(gpu_wait(cb,65535)==GPU_COMPLETED);
   dev_send(7,0,iteration+1);iteration+=1;
 }
 check(read_dseg_page(3)==19&&read_dseg_page(0)==0&&read_dseg_page(1)==1&&read_dseg_page(2)==2);
 halt(0x600d);
}
"#.into()
}

pub fn assets() -> BTreeMap<u32, u8> {
    let mut memory = BTreeMap::new();
    for (i, (rgb, offset)) in [
        (true, 0),
        (true, 8192),
        (true, 16384),
        (false, 0),
        (false, 8192),
    ]
    .into_iter()
    .enumerate()
    {
        let address = ASSET + 128 + i as u32 * 256;
        let mut record = [0u8; 256];
        // Current V96/T64 host-native record, uniform20, normal18, scale_s=0.
        let meta = [
            3u64 | (1 << 8) | (20 << 16) | (5 << 24),
            u64::from(rgb) << 4,
            u64::from(offset as u32),
            65536 | (8u64 << 32) | (1u64 << 40),
        ];
        for (j, v) in meta.into_iter().enumerate() {
            record[j * 8..j * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        let color = if rgb {
            [0x0000ffu32, 0x00ff00, 0xff0000][i]
        } else {
            0
        };
        for (j, xyz) in [[0u32, 0, 0], [16, 0, 0], [0, 16, 0]]
            .into_iter()
            .enumerate()
        {
            let words = [xyz[0] | (xyz[1] << 10) | (xyz[2] << 20), 511 << 20, color];
            for (k, w) in words.into_iter().enumerate() {
                let at = 128 + j * 12 + k * 4;
                record[at..at + 4].copy_from_slice(&w.to_le_bytes());
            }
        }
        record[168..176].copy_from_slice(&(64u64 | (2 << 12)).to_le_bytes());
        for (j, b) in record.into_iter().enumerate() {
            memory.insert(address + j as u32, b);
        }
        let entry = [(address / 128) as u16, 1];
        for (j, b) in entry.into_iter().flat_map(u16::to_le_bytes).enumerate() {
            memory.insert(ASSET + i as u32 * 4 + j as u32, b);
        }
    }
    // One fixed immutable 1x1 RGB565 texture; complete 128-byte allocation.
    for j in 0..128 {
        memory.insert(TEXTURE + j, 0);
    }
    memory.insert(TEXTURE, 0xe0);
    memory.insert(TEXTURE + 1, 0x07);
    memory
}

// Independent exact ray/edge coverage for these power-of-two affine fixtures.
// No DUT state or generated command words enter this reference.
pub fn surface(iteration: u16) -> Vec<u16> {
    let mut result = vec![iteration; 96000];
    let mut paint = |tx: i64, ty: i64, offset: i64, color: u16, blend: bool| {
        let x0 = tx + offset;
        let y0 = ty;
        let z = 65536i64;
        // Edge functions in the GPU's pixel-center ray coordinates.
        for y in 0..240i64 {
            for x in 0..400i64 {
                let px = (2 * x - 399) * z;
                let py = (2 * y - 239) * z;
                let a = px - 512 * x0;
                let b = py - 512 * y0;
                if a >= 0 && b >= 0 && a + b < 512 * 4096 {
                    let old = result[(y * 400 + x) as usize];
                    result[(y * 400 + x) as usize] = if blend {
                        // Black at alpha128 over the old expanded RGB565 color.
                        let r = (u32::from(old >> 11) * 255 + 15) / 31;
                        let g = (u32::from((old >> 5) & 63) * 255 + 31) / 63;
                        let b = (u32::from(old & 31) * 255 + 15) / 31;
                        let quant =
                            |v: u32, n: u32| -> u16 { (((v.div_ceil(2)) * n + 127) / 255) as u16 };
                        (quant(r, 31) << 11) | (quant(g, 63) << 5) | quant(b, 31)
                    } else {
                        color
                    };
                }
            }
        }
    };
    for (offset, c) in [(0, 0xf800), (8192, 0x07e0), (16384, 0x001f)] {
        paint(-32768, -8192, offset, c, false);
    }
    for (offset, c) in [(0, 0x8000), (8192, 0x07e0), (16384, 0x001f)] {
        paint(0, -8192, offset, c, false);
    }
    for offset in [0, 8192] {
        paint(0, 8192, offset, 0x07e0, false);
    }
    paint(-32768, -8192, 0, 0, true);
    result
}
