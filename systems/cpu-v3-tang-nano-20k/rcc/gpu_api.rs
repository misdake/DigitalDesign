#![rcc_resource_library(
    gpu_schema,
    gpu_config,
    gpu_arena,
    gpu_record,
    gpu_unlit,
    gpu_storage,
    gpu_packet,
    gpu_memcpy,
    gpu_bind,
    gpu_mesh,
    gpu_group,
    gpu_execute,
    gpu_present,
    gpu_api_fault,
    gpu_scene_math,
    gpu_source_revision,
    gpu_target,
    gpu_scene_objects,
    gpu_scene_transforms,
    gpu_ui_grid,
    gpu_ui_packet
)]
//! Explicit recording, submission, retirement and prepared scene resources.
use crate::dsl_rt::*;
mod static_assets;
use static_assets::*;
mod static_buffer;
use static_buffer::*;
mod gpu_api_values;
use gpu_api_values::*;
mod gpu_value_constructors;
use gpu_value_constructors::*;
mod gpu_transform_values;
use gpu_transform_values::*;
mod texture_layout;
use texture_layout::*;
mod surface;
use surface::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_config;
use gpu_config::*;
mod gpu_arena;
use gpu_arena::*;
mod gpu_record;
use gpu_record::*;
mod gpu_unlit;
use gpu_unlit::*;
mod gpu_storage;
use gpu_storage::*;
mod gpu_packet;
use gpu_packet::*;
mod gpu_bind;
mod gpu_memcpy;
use gpu_bind::*;
mod gpu_mesh;
use gpu_mesh::*;
mod gpu_group;
use gpu_group::*;
mod gpu_execute;
use gpu_execute::*;
mod gpu_present;
use gpu_present::*;
mod gpu_api_fault;
use gpu_api_fault::*;
mod gpu_target;
use gpu_target::*;

/// A sealed single-frame target, independent of transform/material preparation.
#[derive(Copy, Clone)]
pub struct TargetBinding {
    slot: u16,
    generation_low: u16,
    generation_high: u16,
}

impl CommandMemory {
    pub const fn allocate(command_bytes: usize, push_bytes: usize) -> Self {
        assert!(command_bytes <= 65536 && push_bytes <= 65536);
        let commands = StaticBuffer::new(command_bytes, 128);
        let push = StaticBuffer::new(push_bytes, 128);
        Self::new(
            GpuMemory::from_buffer(commands),
            GpuMemory::from_buffer(push),
        )
    }
}
impl PreparedMemory {
    pub const fn allocate(command_bytes: usize, push_bytes: usize) -> Self {
        let commands = StaticBuffer::new(command_bytes, 128);
        let push = StaticBuffer::new(push_bytes, 128);
        Self::new(
            GpuMemory::from_buffer(commands),
            GpuMemory::from_buffer(push),
        )
    }
}

/// Application-owned prepared commands and push data. Keep referenced external
/// inputs unchanged and allocated until every consuming CB is retired and discarded.
#[rcc_resource]
pub struct PreparedDraws {
    slot: u16,
}
impl PreparedDraws {
    pub fn new(memory: PreparedMemory) -> Self {
        let slot = gpu_api_object(false, GpuObjectKind::Prepared);
        gpu_api_checked(gpu_group_new(slot, memory), GpuCheck::Prepared);
        Self { slot }
    }
    pub fn ptr(&self) -> PreparedDrawsPtr {
        PreparedDrawsPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn record(&mut self) -> PreparedPart {
        gpu_api_checked(gpu_group_part_begin(self.slot), GpuCheck::Prepared);
        PreparedPart {
            slot: GPU_PREPARED_RECORD,
        }
    }
}
impl Drop for PreparedDraws {
    fn drop(&mut self) {
        gpu_api_checked(gpu_group_free(self.slot), GpuCheck::Prepared);
    }
}

#[derive(Copy, Clone)]
pub struct PreparedDrawsPtr {
    handle: GpuObjectHandle,
}
impl PreparedDrawsPtr {
    fn data(self) -> u16 {
        gpu_arena_resolve(self.handle, GpuObjectKind::Prepared)
    }
    pub fn ready(self) -> bool {
        gpu_group_part_ready(self.data())
    }
    pub fn invalidate(self) {
        gpu_api_checked(gpu_group_invalidate(self.data()), GpuCheck::Prepared);
    }
    /// Retire and discard ordinary CBs that reference this part first.
    /// A live command list also prevents recording through this reference.
    pub fn record(self) -> PreparedPart {
        gpu_api_checked(gpu_group_part_begin(self.data()), GpuCheck::Prepared);
        PreparedPart {
            slot: GPU_PREPARED_RECORD,
        }
    }
}

/// Runtime allocation from application-reserved external storage. The pool owns
/// its parts; retire and discard referring CBs before freeing parts or the pool.
#[rcc_resource]
pub struct PreparedPool {
    slot: u16,
}
impl PreparedPool {
    pub fn new(memory: GpuMemory) -> Self {
        let slot = gpu_api_object(false, GpuObjectKind::PreparedPool);
        gpu_api_checked(gpu_group_pool_init(slot, memory), GpuCheck::Prepared);
        Self { slot }
    }
    /// Byte requests round up to 128; 65535 requests the full 64-KiB extent.
    pub fn allocate(&mut self, command_bytes: u16, push_bytes: u16) -> PreparedDrawsPtr {
        let slot = gpu_group_pool_allocate(self.slot, command_bytes, push_bytes);
        gpu_api_checked(slot != 0, GpuCheck::Prepared);
        PreparedDrawsPtr {
            handle: gpu_arena_handle(slot),
        }
    }
    pub fn free(&mut self, part: PreparedDrawsPtr) {
        gpu_api_checked(
            gpu_group_pool_free(self.slot, part.data()),
            GpuCheck::Prepared,
        );
    }
}
impl Drop for PreparedPool {
    fn drop(&mut self) {
        gpu_api_checked(gpu_group_pool_drop(self.slot), GpuCheck::Prepared);
    }
}
impl GpuMemory {
    /// Caller owns both the external bytes and the stable low-memory span for
    /// this transfer. Retire/discard recorded readers and retire Display use
    /// before changing their input bytes; no reference locks are maintained.
    pub unsafe fn write_words(self, offset: u16, source: Array<u16>, count: u16) {
        gpu_api_transfer(self, offset, source, count, false);
    }
    /// Caller has acquired CPU ownership of the source and supplies count words
    /// of writable low-memory storage. Each transfer is at most 128 CPU words.
    pub unsafe fn read_words(self, offset: u16, destination: Array<u16>, count: u16) {
        gpu_api_transfer(self, offset, destination, count, true);
    }
    pub const fn from_buffer(buffer: StaticBuffer) -> Self {
        Self {
            first_sector: buffer.first_sector(),
            sector_count: buffer.sector_count(),
        }
    }
    pub const fn from_surface(surface: Rgb565Surface) -> Self {
        Self {
            first_sector: surface.first_sector(),
            sector_count: surface.sector_count(),
        }
    }
    /// The caller preserves the placed external allocation and its access lifetime.
    pub const unsafe fn from_asset(asset: AssetBytes) -> Self {
        assert!(asset.sector_count != 0);
        assert!(asset.sector_count - 1 <= 65535 - asset.first_sector);
        Self {
            first_sector: asset.first_sector,
            sector_count: asset.sector_count,
        }
    }
    /// Caller guarantees real allocation, lifetime, and no illegal aliasing.
    pub unsafe fn from_sectors(first_sector: u16, sector_count: u16) -> Self {
        if !gpu_memory_extent_valid(first_sector, sector_count) {
            gpu_api_fail(GpuFault::MemoryExtent, first_sector, sector_count);
        }
        GpuMemory {
            first_sector: first_sector,
            sector_count: sector_count,
        }
    }
}
fn gpu_api_transfer(memory: GpuMemory, offset: u16, data: Array<u16>, count: u16, read: bool) {
    if count == 0 {
        return;
    }
    let relative = offset >> 6;
    if count > GPU_API_PUSH_CHUNK_WORDS
        || relative >= memory.sector_count
        || ((offset & 63) + count - 1) >> 6 >= memory.sector_count - relative
    {
        gpu_api_fail(GpuFault::MemoryRange, offset, count);
    }
    if !gpu_storage_copy_owned(data, memory.first_sector, offset, count, read) {
        gpu_api_fail(GpuFault::MemoryTransfer, offset, count);
    }
}
impl TextureDesc {
    pub const fn from_memory(memory: GpuMemory, layout: TextureLayout) -> Self {
        debug_assert!(memory.first_sector & 1 == 0);
        debug_assert!(memory.sector_count >= layout.required_sectors());
        Self {
            memory,
            size_log2: layout.size_log2,
            full_mip: layout.full_mip,
            format: layout.format,
        }
    }
    /// Bind immutable placed bytes. Layout validates the complete native footprint.
    pub const unsafe fn from_asset(asset: AssetBytes, layout: TextureLayout) -> Self {
        let required = layout.required_sectors();
        let high = required >> 9;
        let low = (required & 511) << 7;
        debug_assert!(
            asset.byte_len_hi > high || (asset.byte_len_hi == high && asset.byte_len_lo >= low)
        );
        let memory = unsafe { GpuMemory::from_asset(asset) };
        Self::from_memory(memory, layout)
    }
}
impl MeshDesc {
    /// Word offset within the supplied native package, not an absolute address.
    pub const fn from_memory(
        memory: GpuMemory,
        directory_offset_words: u16,
        meshlets: u16,
    ) -> Self {
        debug_assert!(meshlets <= 5460);
        let relative = directory_offset_words >> 6;
        let word = directory_offset_words & 63;
        debug_assert!(relative < memory.sector_count);
        debug_assert!((word + meshlets * 2 + 63) >> 6 <= memory.sector_count - relative);
        Self {
            directory_sector: memory.first_sector + relative,
            directory_word: word,
            meshlet_count: meshlets,
            storage: memory,
        }
    }
    /// Compile-time byte offset supports directories anywhere in a placed package.
    pub const unsafe fn from_asset(
        asset: AssetBytes,
        directory_offset_bytes: usize,
        meshlets: u16,
    ) -> Self {
        let length = ((asset.byte_len_hi as usize) << 16) | asset.byte_len_lo as usize;
        debug_assert!(meshlets <= 5460 && directory_offset_bytes & 1 == 0);
        debug_assert!(directory_offset_bytes < length);
        debug_assert!(directory_offset_bytes + (meshlets as usize) * 4 <= length);
        let memory = unsafe { GpuMemory::from_asset(asset) };
        Self {
            directory_sector: memory.first_sector + (directory_offset_bytes / 128) as u16,
            directory_word: ((directory_offset_bytes % 128) / 2) as u16,
            meshlet_count: meshlets,
            storage: memory,
        }
    }
}
#[derive(Copy, Clone)]
pub struct Rgb565 {
    pub raw: u16,
}
impl Rgb565 {
    pub const fn from_raw(raw: u16) -> Self {
        Rgb565 { raw: raw }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::from_raw((((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3))
    }
    pub const fn gray(value: u8) -> Self {
        Self::rgb(value, value, value)
    }
}
#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum LoadOp {
    Load,
    Clear,
}
pub struct ColorLoad {
    pub op: LoadOp,
    pub value: Rgb565,
}
impl ColorLoad {
    pub const fn load() -> Self {
        ColorLoad {
            op: LoadOp::Load,
            value: Rgb565 { raw: 0 },
        }
    }
    pub const fn clear(value: Rgb565) -> Self {
        ColorLoad {
            op: LoadOp::Clear,
            value: value,
        }
    }
}
pub struct DepthLoad {
    pub op: LoadOp,
    pub value: u16,
}
impl DepthLoad {
    pub const fn clear_far() -> Self {
        Self::clear(65535)
    }
    pub const fn load() -> Self {
        DepthLoad {
            op: LoadOp::Load,
            value: 0,
        }
    }
    pub const fn clear(value: u16) -> Self {
        DepthLoad {
            op: LoadOp::Clear,
            value: value,
        }
    }
}

#[rcc_resource]
pub struct Gpu {
    identity: u16,
}
#[rcc_resource]
pub struct CommandBuffer {
    slot: u16,
}
/// An ordered submission table stored directly in CPU-visible SDRAM.
#[rcc_resource]
pub struct CommandList {
    allocation: u16,
}
const GPU_LIST_COUNT: u16 = 0;
const GPU_LIST_USED: u16 = 1;
const GPU_LIST_SLOTS: u16 = 2;
const GPU_LIST_SCENE_SLOT: u16 = 3;
#[rcc_resource(parent=CommandBuffer)]
pub struct Pass {
    slot: u16,
}
/// A transient prepared recorder with an exclusive recording state.
#[rcc_resource]
pub struct PreparedPart {
    slot: u16,
}
#[rcc_resource]
pub struct ColorBuffer {
    slot: u16,
}
#[rcc_resource]
pub struct DepthBuffer {
    slot: u16,
}
#[rcc_resource]
pub struct Mesh {
    slot: u16,
}
#[rcc_resource]
pub struct Display {
    identity: u16,
}

impl Gpu {
    pub fn init(config: GpuConfig) -> Self {
        if !gpu_config_init(config) {
            gpu_api_fail(GpuFault::Initialization, 0, 0);
        }
        Gpu { identity: 1 }
    }
    pub fn command_buffer(&mut self) -> CommandBuffer {
        let slot = gpu_config_take_buffer();
        if slot == 65535 {
            gpu_api_fail(GpuFault::CommandBufferUnavailable, 0, 0);
        }
        CommandBuffer { slot: slot }
    }
    pub fn submit(&mut self, buffer: CommandBuffer) {
        let slot = buffer.slot;
        resource_disarm(buffer);
        gpu_api_submit(slot);
    }
    /// Consume and submit the list as an ordered batch.
    pub fn submit_list(&mut self, list: CommandList) {
        let allocation = list.allocation;
        let meta = gpu_api_list_metadata(allocation);
        let count = meta[GPU_LIST_COUNT];
        if meta[GPU_LIST_USED] != count {
            gpu_api_fail(GpuFault::CommandList, meta[GPU_LIST_USED], count);
        }
        resource_disarm(list);
        if !gpu_execute_submit_list(allocation, count) {
            gpu_api_fail(
                GpuFault::SubmissionRejected,
                GPU_EXECUTION.error as u16,
                count,
            );
        }
    }
    pub fn try_wait(&mut self) -> Option<CommandBuffer> {
        if GPU_EXECUTION.owner == GPU_LIST_OWNER {
            return None;
        }
        let completed = gpu_execute_poll();
        if completed == 65535 {
            gpu_api_fail(
                GpuFault::ExecutionFault,
                GPU_EXECUTION.error as u16,
                GPU_EXECUTION.owner,
            );
        }
        if completed == 0 {
            return None;
        }
        let buffer = CommandBuffer {
            slot: completed - 1,
        };
        Some(buffer)
    }
    pub fn wait(&mut self) -> CommandBuffer {
        if GPU_EXECUTION.phase == ExecutionPhase::Fault {
            gpu_api_fail(
                GpuFault::ExecutionFault,
                GPU_EXECUTION.error as u16,
                GPU_EXECUTION.owner,
            );
        }
        if GPU_EXECUTION.phase != ExecutionPhase::Running || GPU_EXECUTION.owner == GPU_LIST_OWNER {
            gpu_api_fail(
                GpuFault::WaitWithoutSubmission,
                GPU_EXECUTION.phase as u16,
                GPU_EXECUTION.owner,
            );
        }
        let mut completed = gpu_execute_poll();
        while completed == 0 {
            completed = gpu_execute_poll();
        }
        if completed == 65535 {
            gpu_api_fail(
                GpuFault::ExecutionFault,
                GPU_EXECUTION.error as u16,
                GPU_EXECUTION.owner,
            );
        }
        CommandBuffer {
            slot: completed - 1,
        }
    }
    /// Reclaim a completed list and make any consumed CB slots available again.
    pub fn try_wait_list(&mut self) -> bool {
        gpu_api_poll_list()
    }
    pub fn wait_list(&mut self) {
        if GPU_EXECUTION.owner != GPU_LIST_OWNER {
            gpu_api_fail(
                GpuFault::WaitWithoutSubmission,
                GPU_EXECUTION.phase as u16,
                GPU_EXECUTION.owner,
            );
        }
        while !gpu_api_poll_list() {}
    }
}
fn gpu_api_submit(slot: u16) {
    if !gpu_execute_submit(slot) {
        gpu_api_fail(
            GpuFault::SubmissionRejected,
            GPU_EXECUTION.error as u16,
            slot,
        );
    }
}
fn gpu_api_poll_list() -> bool {
    if GPU_EXECUTION.owner != GPU_LIST_OWNER {
        return false;
    }
    let completed = gpu_execute_poll();
    if completed == 65535 {
        gpu_api_fail(
            GpuFault::ExecutionFault,
            GPU_EXECUTION.error as u16,
            GPU_EXECUTION.owner,
        );
    }
    if completed == 0 {
        return false;
    }
    gpu_api_release_list(GPU_EXECUTION.list_allocation);
    GPU_EXECUTION.list_allocation = 0;
    true
}
fn gpu_api_release_list(allocation: u16) {
    let meta = gpu_api_list_metadata(allocation);
    let slots = meta[GPU_LIST_SLOTS];
    let mut slot: u16 = 0;
    while slot < 2 {
        let mask: u16 = 1 << slot;
        if slots & mask != 0 {
            if !gpu_record_reset(slot) {
                gpu_api_fail(GpuFault::RecorderReset, slot, 0);
            }
            GPU_EXECUTION.taken_slots &= 65535 ^ mask;
        }
        slot += 1;
    }
    let parts = gpu_api_list_parts(allocation);
    let count = meta[GPU_LIST_USED];
    let mut index: u16 = 0;
    while index < count {
        let owner = parts[index];
        if owner != 0 {
            let mut data = gpu_arena_data(owner);
            data[GPU_PREPARED_LIST_REFS] -= 1;
        }
        index += 1;
    }
    free(Ptr::from_addr(allocation));
}

impl CommandList {
    /// Declare the final segment count so entries can be placed right-aligned
    /// as they are added. The table can then be submitted without a copy.
    pub fn new(segment_count: u16) -> Self {
        if segment_count == 0 || segment_count > GPU_LIST_MAX_SEGMENTS {
            gpu_api_fail(GpuFault::CommandList, segment_count, GPU_LIST_MAX_SEGMENTS);
        }
        let words = GPU_LIST_METADATA_WORDS + 2 * GPU_LIST_TABLE_WORDS - 1 + segment_count;
        let allocation = try_malloc(words).addr();
        if allocation == 0 {
            gpu_api_fail(GpuFault::CommandList, segment_count, 0);
        }
        let mut meta = gpu_api_list_metadata(allocation);
        meta[GPU_LIST_COUNT] = segment_count;
        meta[GPU_LIST_USED] = 0;
        meta[GPU_LIST_SLOTS] = 0;
        meta[GPU_LIST_SCENE_SLOT] = 0;
        Self { allocation }
    }

    /// Consume one recorded CB. It must fit in one segment (8 KiB).
    pub fn push(&mut self, buffer: CommandBuffer) {
        let slot = buffer.slot;
        if !gpu_record_seal(slot) {
            gpu_api_fail(GpuFault::CommandList, slot, 0);
        }
        let record = gpu_record_data(slot);
        let memory = GpuMemory {
            first_sector: record[GPU_RECORD_COMMAND_BASE],
            sector_count: record[GPU_RECORD_COMMAND_CAPACITY] >> 6,
        };
        gpu_api_list_append(
            self.allocation,
            memory,
            0,
            record[GPU_RECORD_COMMAND_USED] >> 1,
            0,
        );
        let mut meta = gpu_api_list_metadata(self.allocation);
        meta[GPU_LIST_SLOTS] |= 1u16 << slot;
        meta[GPU_LIST_SCENE_SLOT] = if record[GPU_RECORD_TARGET_WORD] != 0
            && record[GPU_CB_VIEW_WORD] != 0
            && gpu_bind_scene_current(slot)
        {
            slot + 1
        } else {
            0
        };
        resource_disarm(buffer);
    }

    /// Append a raw command span. Offsets and lengths count 32-bit commands.
    /// The caller supplies valid commands, entry state, synchronization and END
    /// for the final span, and retains all backing data until list retirement.
    pub unsafe fn push_slice(&mut self, commands: GpuMemory, first: u16, length: u16) {
        gpu_api_list_append(self.allocation, commands, first, length, 0);
        let mut meta = gpu_api_list_metadata(self.allocation);
        meta[GPU_LIST_SCENE_SLOT] = 0;
    }

    /// Replay whole marked draw groups in `[first, end)` from a finished part.
    /// A recorded CB establishes Target and View before the range, and a later
    /// segment ends the batch. The list protects the part until it retires or is dropped.
    pub fn push_prepared_range(&mut self, part: PreparedDrawsPtr, first: u16, end: u16) {
        let owner = part.data();
        let meta = gpu_api_list_metadata(self.allocation);
        if meta[GPU_LIST_USED] + 1 >= meta[GPU_LIST_COUNT]
            || !gpu_group_slice_valid(owner, first, end, meta[GPU_LIST_SCENE_SLOT])
        {
            gpu_api_fail(GpuFault::CommandList, first, end);
        }
        let mut data = gpu_arena_data(owner);
        let commands = GpuMemory {
            first_sector: data[GPU_PREPARED_COMMAND],
            sector_count: data[GPU_PREPARED_COMMAND_CAPACITY] >> 6,
        };
        gpu_api_list_append(self.allocation, commands, first, end - first, owner);
        data[GPU_PREPARED_LIST_REFS] += 1;
    }
}
impl Drop for CommandList {
    fn drop(&mut self) {
        gpu_api_release_list(self.allocation);
    }
}
fn gpu_api_list_metadata(allocation: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(allocation).as_u16_array() }
}
fn gpu_api_list_parts(allocation: u16) -> Array<u16> {
    // CPU-only part references follow the DMA-visible table.
    let address = gpu_list_table_address(allocation) + GPU_LIST_TABLE_WORDS;
    unsafe { Ptr::from_addr(address).as_u16_array() }
}
fn gpu_api_list_append(allocation: u16, commands: GpuMemory, first: u16, length: u16, owner: u16) {
    let mut meta = gpu_api_list_metadata(allocation);
    let count = meta[GPU_LIST_COUNT];
    let used = meta[GPU_LIST_USED];
    let first_sector = first >> 5;
    let first_slot = first & 31;
    if used >= count || length == 0 || length > 2048 || first_sector >= commands.sector_count {
        gpu_api_fail(GpuFault::CommandList, first, length);
    }
    let last = first_slot + length - 1;
    let sectors_minus_one = last >> 5;
    if sectors_minus_one >= 64 || sectors_minus_one >= commands.sector_count - first_sector {
        gpu_api_fail(GpuFault::CommandList, first, length);
    }
    let mut table = unsafe { Ptr::from_addr(gpu_list_table_address(allocation)).as_u16_array() };
    let index = (GPU_LIST_MAX_SEGMENTS - count + used) << 1;
    table[index] = commands.first_sector + first_sector;
    table[index + 1] = first_slot | ((last & 31) << 5) | (sectors_minus_one << 10);
    let mut parts = gpu_api_list_parts(allocation);
    parts[used] = owner;
    meta[GPU_LIST_USED] = used + 1;
}
impl Drop for Gpu {
    // No implicit wait, context reset or reclamation of a driver-owned CB.
    fn drop(&mut self) {
        GPU_EXECUTION.controller = 0;
    }
}
fn gpu_api_new_surface(memory: GpuMemory, kind: GpuObjectKind) -> u16 {
    let slot = gpu_arena_alloc(false, kind);
    if slot == 0 {
        gpu_api_fail(GpuFault::SurfaceAllocation, kind as u16, 0);
    }
    if !gpu_surface_init(gpu_arena_data(slot), memory) {
        gpu_api_fail(
            GpuFault::SurfaceDescription,
            memory.first_sector(),
            memory.sector_count(),
        );
    }
    slot
}
impl ColorBuffer {
    pub fn from_surface(surface: Rgb565Surface) -> Self {
        Self::from_memory(GpuMemory::from_surface(surface))
    }
    pub fn from_memory(memory: GpuMemory) -> Self {
        ColorBuffer {
            slot: gpu_api_new_surface(memory, GpuObjectKind::Color),
        }
    }

    pub fn texture_view(&self) -> TextureView {
        let source = gpu_arena_data(self.slot);
        let memory = unsafe { GpuMemory::from_sectors(source[0u16], source[1u16]) };
        let view = TextureView {
            words: Buf::new([0; 6]),
        };
        gpu_api_checked(gpu_scene_color_view(view.data(), memory), GpuCheck::View);
        view
    }

    pub fn ptr(&self) -> ColorBufferPtr {
        ColorBufferPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl DepthBuffer {
    pub fn from_surface(surface: DepthSurface) -> Self {
        Self::from_memory(GpuMemory::from_buffer(surface.storage))
    }
    pub fn from_memory(memory: GpuMemory) -> Self {
        DepthBuffer {
            slot: gpu_api_new_surface(memory, GpuObjectKind::Depth),
        }
    }

    pub fn ptr(&self) -> DepthBufferPtr {
        DepthBufferPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for ColorBuffer {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GpuObjectKind::Color) {
            gpu_api_fail(
                GpuFault::SurfaceRelease,
                GpuObjectKind::Color as u16,
                self.slot,
            );
        }
    }
}
impl Drop for DepthBuffer {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GpuObjectKind::Depth) {
            gpu_api_fail(
                GpuFault::SurfaceRelease,
                GpuObjectKind::Depth as u16,
                self.slot,
            );
        }
    }
}
impl Mesh {
    /// Refresh only directory descriptors for future recording. Previously
    /// recorded commands retain their snapshots; retire payload users first.
    pub fn refresh_directory(&mut self) {
        gpu_api_checked(
            gpu_mesh_refresh_directory(gpu_arena_data(self.slot)),
            GpuCheck::Mesh,
        );
    }
    pub fn from_native(desc: MeshDesc) -> Self {
        let slot = gpu_arena_alloc(false, GpuObjectKind::Mesh);
        if slot == 0 {
            gpu_api_fail(GpuFault::MeshAllocation, 0, 0);
        }
        if !gpu_mesh_init(gpu_arena_data(slot), desc) {
            gpu_api_fail(GpuFault::MeshDescription, 0, 0);
        }
        Mesh { slot: slot }
    }
    pub fn subset(&self, first: u16, count: u16) -> Mesh {
        let slot = gpu_arena_alloc(false, GpuObjectKind::Mesh);
        if slot == 0 {
            gpu_api_fail(GpuFault::MeshAllocation, 0, 0);
        }
        if !gpu_mesh_subset(
            gpu_arena_data(slot),
            gpu_arena_data(self.slot),
            first,
            count,
        ) {
            gpu_api_fail(GpuFault::MeshRange, first, count);
        }
        Mesh { slot: slot }
    }
    pub fn meshlet_count(&self) -> u16 {
        let ptr = self.ptr();
        ptr.meshlet_count()
    }

    pub fn ptr(&self) -> MeshPtr {
        MeshPtr {
            handle: gpu_arena_handle(self.slot),
            first: 0,
            count: gpu_arena_data(self.slot)[GPU_MESH_COUNT],
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for Mesh {
    fn drop(&mut self) {
        gpu_mesh_release_directory(gpu_arena_data(self.slot));
        if !gpu_arena_free(self.slot, false, GpuObjectKind::Mesh) {
            gpu_api_fail(GpuFault::MeshRelease, self.slot, 0);
        }
    }
}
fn gpu_api_copy_plane(slot: u16, s: Array<u16>, d: Array<u16>, kind: ResourceFactKind) {
    gpu_unlit_end(slot);
    if gpu_record_overlap(s[0u16], s[1u16], d[0u16], d[1u16]) {
        gpu_api_fail(GpuFault::RecorderOperation, slot, 0);
    }
    let facts: Buf<u16, 8> = Buf::new([
        s[0u16],
        s[1u16],
        kind as u16,
        GPU_FACT_READ,
        d[0u16],
        d[1u16],
        kind as u16,
        GPU_FACT_WRITE,
    ]);
    let fact_plan: Buf<u16, 2> = Buf::new([0; 2]);
    if !gpu_record_resource_room(slot, facts.as_array(), 2, fact_plan.as_array()) {
        gpu_api_fail(GpuFault::ResourceConflict, slot, 0);
    }
    if !gpu_packet_copy_plane(slot, s[0u16], d[0u16]) {
        gpu_api_fail(GpuFault::RecorderOperation, slot, 0);
    }
    gpu_record_resource_commit(slot, facts.as_array(), 2, fact_plan.as_array());
    let mut c = gpu_record_data(slot);
    c[GPU_RECORD_COPY_FACT] = if kind == ResourceFactKind::Color {
        fact_plan[0u16] + 1
    } else {
        0
    };
}
impl CommandBuffer {
    /// Copy the 400x256 physical RGB565 plane between nonoverlapping surfaces.
    /// Records source-read/destination-write ownership; allocation tails remain
    /// untouched. Call between passes. Submission publication/leases are shared
    /// with drawing. Target-only rebinding can relocate one leading color copy.
    pub fn copy_color(&mut self, source: ColorBufferPtr, destination: ColorBufferPtr) {
        gpu_api_copy_plane(
            self.slot,
            source.data(),
            destination.data(),
            ResourceFactKind::Color,
        );
    }
    /// Copy the physical depth plane without changing its stored values.
    /// The caller retains a completed source until this CB retires.
    pub fn copy_depth(&mut self, source: DepthBufferPtr, destination: DepthBufferPtr) {
        gpu_api_copy_plane(
            self.slot,
            source.data(),
            destination.data(),
            ResourceFactKind::Depth,
        );
    }
    /// Seal one target block, with any immutable draw groups and at most one
    /// leading color copy. No matrices, state or source objects are patched.
    pub fn target_binding(&mut self) -> TargetBinding {
        gpu_api_checked(gpu_target_binding(self.slot), GpuCheck::TargetBinding);
        let c = gpu_record_data(self.slot);
        TargetBinding {
            slot: self.slot,
            generation_low: c[GPU_RECORD_GENERATION_LOW],
            generation_high: c[GPU_RECORD_GENERATION_HIGH],
        }
    }
    /// Change a CPU-owned frame's target and optional copy destination together.
    /// A pending Display swap or overlap with its current Front rejects before
    /// any payload/fact change. Submission publishes writes and checks execution ownership.
    pub fn retarget_target(
        &mut self,
        binding: TargetBinding,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        gpu_api_checked(self.slot == binding.slot, GpuCheck::TargetBinding);
        gpu_api_checked(
            gpu_target_retarget(
                self.slot,
                binding.generation_low,
                binding.generation_high,
                color.data(),
                depth.data(),
            ),
            GpuCheck::TargetBinding,
        );
    }
    pub fn reset(&mut self) {
        if !gpu_record_reset(self.slot) {
            gpu_api_fail(GpuFault::RecorderReset, self.slot, 0);
        }
    }
    pub fn pass(
        &mut self,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
        color_load: ColorLoad,
        depth_load: DepthLoad,
    ) -> Pass {
        let c = color.data();
        let d = depth.data();
        let mut mask: u16 = 0;
        if color_load.op == LoadOp::Clear {
            mask |= 1;
        }
        if depth_load.op == LoadOp::Clear {
            mask |= 2;
        }
        let facts: Buf<u16, 8> = Buf::new([
            c[0u16],
            c[1u16],
            ResourceFactKind::Color as u16,
            GPU_FACT_READ | GPU_FACT_WRITE,
            d[0u16],
            d[1u16],
            ResourceFactKind::Depth as u16,
            GPU_FACT_READ | GPU_FACT_WRITE,
        ]);
        let fact_plan: Buf<u16, 2> = Buf::new([0; 2]);
        if !gpu_record_resource_room(self.slot, facts.as_array(), 2, fact_plan.as_array()) {
            gpu_api_fail(GpuFault::ResourceConflict, self.slot, 0);
        }
        if !gpu_packet_begin(
            self.slot,
            c[0u16],
            d[0u16],
            mask,
            color_load.value.raw,
            depth_load.value,
        ) {
            gpu_api_fail(GpuFault::RecorderOperation, self.slot, mask);
        }
        gpu_record_resource_commit(self.slot, facts.as_array(), 2, fact_plan.as_array());
        let mut record = gpu_record_data(self.slot);
        record[GPU_RECORD_COLOR_FACT] = fact_plan[0u16] + 1;
        record[GPU_RECORD_DEPTH_FACT] = fact_plan[1u16] + 1;
        Pass { slot: self.slot }
    }
    pub fn invalidate_textures(&mut self) {
        if !gpu_packet_invalidate_textures(self.slot) {
            gpu_api_fail(GpuFault::PassPhase, self.slot, 0);
        }
    }
    pub fn command_words(&self) -> u16 {
        gpu_record_data(self.slot)[GPU_RECORD_COMMAND_USED]
    }
    pub fn push_words(&self) -> u16 {
        gpu_record_data(self.slot)[GPU_RECORD_PUSH_USED]
    }

    /// Reserve the bank prefix in an empty CB before opening its Pass.
    pub fn prepare_unlit(
        &mut self,
        bank: &mut UnlitBank,
        camera: CameraPtr,
        transform: TransformPtr,
    ) {
        gpu_api_checked(
            gpu_unlit_prepare(
                self.slot,
                unsafe { Ptr::from_addr(bank.data).as_u16_array() },
                camera.data(),
                transform.data(),
            ),
            GpuCheck::Mesh,
        );
    }
    /// Update View and Target for a completed bank-only recording.
    pub fn patch_unlit(
        &mut self,
        bank: &UnlitBank,
        camera: CameraPtr,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        gpu_api_checked(
            gpu_unlit_patch(
                self.slot,
                unsafe { Ptr::from_addr(bank.data).as_u16_array() },
                camera.data(),
                color.data(),
                depth.data(),
            ),
            GpuCheck::TargetBinding,
        );
    }
}
impl Drop for CommandBuffer {
    fn drop(&mut self) {
        if !gpu_config_drop_buffer(self.slot) {
            gpu_api_fail(GpuFault::CameraBinding, self.slot, 0);
        }
    }
}
impl Pass {
    /// Append an immutable part while retaining the next immediate draw's state.
    pub fn draw_prepared(&mut self, part: PreparedDrawsPtr) {
        gpu_unlit_end(self.slot);
        gpu_api_checked(
            gpu_group_part_append(self.slot, part.data()),
            GpuCheck::Prepared,
        );
    }
    pub fn invalidate_textures(&mut self) {
        gpu_api_checked(
            gpu_record_data(self.slot)[GPU_GROUP_RECORDING] == 0,
            GpuCheck::Prepared,
        );
        if !gpu_packet_invalidate_textures(self.slot) {
            gpu_api_fail(GpuFault::PassPhase, self.slot, 0);
        }
    }

    pub fn set_camera(&mut self, camera: CameraPtr) {
        gpu_api_set_camera(self.slot, camera);
    }
    pub fn set_state(&mut self, state: RenderStatePtr) {
        gpu_api_set_state(self.slot, state);
    }
    pub fn draw(
        &mut self,
        transform: TransformPtr,
        mesh: MeshPtr,
        material: MaterialPtr,
        view: &TextureView,
    ) {
        gpu_api_draw_textured(self.slot, transform, mesh, material, view);
    }
    pub fn draw_colored(&mut self, transform: TransformPtr, mesh: MeshPtr, material: MaterialPtr) {
        gpu_api_draw_colored(self.slot, transform, mesh, material);
    }
    /// Draw unlit native Billboard meshlets with offsets in units of 1/512.
    pub fn draw_billboard(
        &mut self,
        transform: TransformPtr,
        mesh: MeshPtr,
        material: MaterialPtr,
        view: &TextureView,
        facing: BillboardFacing,
    ) {
        gpu_api_draw(
            self.slot,
            gpu_arena_resolve(transform.handle, GpuObjectKind::Transform),
            mesh,
            gpu_arena_resolve(material.handle, GpuObjectKind::Material),
            view.words.as_ptr(),
            facing as u16 + 1,
        );
    }
    pub fn draw_ui(&mut self, grid: UiGridPtr, clip: CellRect) {
        gpu_unlit_end(self.slot);
        gpu_api_checked(
            gpu_record_data(self.slot)[GPU_GROUP_RECORDING] == 0,
            GpuCheck::Prepared,
        );
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        let data = grid.data();
        let facts: Buf<u16, 8> = Buf::new([
            data[GPU_UI_GRID_BASE],
            data[GPU_UI_GRID_COUNT],
            ResourceFactKind::Grid as u16,
            GPU_FACT_READ,
            data[GPU_UI_GRID_ATLAS_BASE],
            data[GPU_UI_GRID_ATLAS_COUNT],
            ResourceFactKind::Atlas as u16,
            GPU_FACT_READ,
        ]);
        let fact_plan: Buf<u16, 2> = Buf::new([0; 2]);
        gpu_api_checked(
            gpu_record_resource_room(self.slot, facts.as_array(), 2, fact_plan.as_array()),
            GpuCheck::UiGrid,
        );
        let native = unsafe {
            gpu_native_scratch()
                .as_ptr()
                .add(GPU_SCRATCH_UI as i16)
                .as_u16_array()
        };
        gpu_api_checked(gpu_ui_descriptor_prepare(data, native), GpuCheck::UiGrid);
        gpu_api_checked(
            gpu_ui_packet_draw(self.slot, native, clip.x, clip.y, clip.width, clip.height),
            GpuCheck::UiGrid,
        );
        gpu_record_resource_commit(self.slot, facts.as_array(), 2, fact_plan.as_array());
    }

    /// Draw a meshlet range with the prepared Transform and View and a fixed binding.
    /// Ordinary draw, prepared part, UI or copy commands end the bank phase.
    pub fn draw_unlit(&mut self, bank: &UnlitBank, binding: u16, first: u16, count: u16) {
        gpu_api_checked(
            gpu_unlit_draw(
                self.slot,
                unsafe { Ptr::from_addr(bank.data).as_u16_array() },
                binding,
                first,
                count,
            ),
            GpuCheck::Mesh,
        );
    }
}
impl Drop for Pass {
    fn drop(&mut self) {
        if !gpu_record_close_pass(self.slot) {
            gpu_api_fail(GpuFault::OpenPassDrop, self.slot, 0);
        }
    }
}
impl Display {
    pub fn open() -> Self {
        Self::open_with_format(DisplayFormat::Rgb565)
    }
    pub fn open_with_format(format: DisplayFormat) -> Self {
        if !gpu_present_open_with_format(format) {
            gpu_api_fail(GpuFault::DisplayOpen, 0, 0);
        }
        Display { identity: 1 }
    }
    pub fn frame_index(&self) -> u16 {
        dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX)
    }
    pub fn set_next_swap(&mut self, color: ColorBufferPtr) {
        if !gpu_present_set_next(color.data()[0u16]) {
            gpu_api_fail(GpuFault::DisplaySwap, 0, 0);
        }
    }
    /// Stage the pixel interpretation together with its completed framebuffer.
    pub fn set_next_swap_with_format(&mut self, color: ColorBufferPtr, format: DisplayFormat) {
        if !gpu_present_set_next_with_format(color.data()[0u16], format) {
            gpu_api_fail(GpuFault::DisplaySwap, 0, 0);
        }
    }
    pub fn wait_vblank(&mut self) {
        if !gpu_present_wait_vblank() {
            gpu_api_fail(GpuFault::DisplayWait, 0, 0);
        }
    }
    // One observation of the queued swap, including old-read retirement.
    // With no owned request this reports stable/idle, not a new frame edge.
    pub fn try_wait_swap(&mut self) -> bool {
        let status = gpu_present_poll();
        if status == PresentPoll::Fault {
            gpu_api_fail(GpuFault::DisplayWait, 0, 0);
        }
        status != PresentPoll::Pending
    }
}
impl Drop for Display {
    fn drop(&mut self) {
        // Keep pending request metadata until hardware retirement is observed.
        GPU_PRESENT.ready = 0;
    }
}

// Read-only capture consumer bridge; this never owns or submits a buffer.
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;
mod gpu_scene_transforms;
use gpu_scene_transforms::*;
mod gpu_source_revision;
use gpu_source_revision::*;
mod gpu_ui_grid;
use gpu_ui_grid::*;
mod gpu_ui_packet;
use gpu_ui_packet::*;

// Check contexts are wire diagnostics, not arena resource kinds.
#[repr(u16)]
#[derive(Copy, Clone)]
enum GpuCheck {
    Camera = 1,
    Transform = 2,
    Material = 3,
    State = 4,
    Mesh = 5,
    Texture = 6,
    View = 7,
    Color = 8,
    Depth = 9,
    UiAtlas = 10,
    UiGrid = 11,
    TargetBinding = 14,
    Prepared = 15,
}

fn gpu_api_object(large: bool, kind: GpuObjectKind) -> u16 {
    let slot = gpu_arena_alloc(large, kind);
    if slot == 0 {
        gpu_api_fail(GpuFault::ObjectAllocation, kind as u16, 0);
    }
    slot
}
fn gpu_api_checked(ok: bool, context: GpuCheck) {
    if !ok {
        gpu_api_fail(GpuFault::ObjectValidation, context as u16, 0);
    }
}
impl Projection {
    pub const fn from_focal_pixels(focal: fix32, far: fix32) -> Self {
        let result: ProjectionResult = gpu_scene_projection_focal_desc(focal, far);
        assert!(result.valid);
        result.value
    }
    pub const fn from_vertical_fov(angle: fix32, far: fix32) -> Self {
        let result: ProjectionResult = gpu_scene_projection_fov_desc(angle, far, false);
        assert!(result.valid);
        result.value
    }
    pub const fn from_horizontal_fov(angle: fix32, far: fix32) -> Self {
        let result: ProjectionResult = gpu_scene_projection_fov_desc(angle, far, true);
        assert!(result.valid);
        result.value
    }
}

#[rcc_resource]
pub struct Camera {
    slot: u16,
}
impl Camera {
    pub fn new(desc: CameraDesc) -> Self {
        let slot = gpu_api_object(true, GpuObjectKind::Camera);
        gpu_api_checked(
            gpu_scene_camera_init(gpu_arena_data(slot), desc),
            GpuCheck::Camera,
        );
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(slot)), GpuCheck::Camera);
        Camera { slot: slot }
    }

    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3, projection: Projection) -> Self {
        let slot = gpu_api_object(true, GpuObjectKind::Camera);
        gpu_api_checked(
            gpu_scene_camera_init_look_at(gpu_arena_data(slot), eye, target, up, projection),
            GpuCheck::Camera,
        );
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(slot)), GpuCheck::Camera);
        Camera { slot: slot }
    }
    pub fn view(&self) -> Mat3x4 {
        gpu_api_camera_view(gpu_arena_data(self.slot))
    }

    pub fn set_pose(&mut self, eye: Vec3, angles: Vec3) {
        gpu_api_camera_set_pose(gpu_arena_data(self.slot), eye, angles)
    }

    pub fn set_projection(&mut self, projection: Projection) {
        gpu_api_camera_set_projection(gpu_arena_data(self.slot), projection)
    }

    pub fn set_look_at(&mut self, eye: Vec3, target: Vec3, up: Vec3) {
        gpu_api_camera_set_look_at(gpu_arena_data(self.slot), eye, target, up)
    }

    pub fn set_look_at_y_up(&mut self, eye: Vec3, target: Vec3) {
        gpu_api_camera_set_look_at_y_up(gpu_arena_data(self.slot), eye, target)
    }

    pub fn set_look_to(&mut self, eye: Vec3, direction: Vec3, up: Vec3) {
        gpu_api_camera_set_look_to(gpu_arena_data(self.slot), eye, direction, up)
    }

    pub fn set_view(&mut self, view: Mat3x4) {
        gpu_api_camera_set_view(gpu_arena_data(self.slot), view)
    }

    pub fn ptr(&self) -> CameraPtr {
        CameraPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for Camera {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, true, GpuObjectKind::Camera) {
            gpu_api_fail(
                GpuFault::ObjectRelease,
                GpuObjectKind::Camera as u16,
                self.slot,
            );
        }
    }
}

#[rcc_resource]
pub struct Transform {
    slot: u16,
}
impl Transform {
    pub fn new(pose: Pose) -> Self {
        let slot = gpu_api_object(true, GpuObjectKind::Transform);
        gpu_api_checked(
            gpu_scene_model_init(gpu_arena_data(slot), pose),
            GpuCheck::Transform,
        );
        gpu_api_checked(
            gpu_model_revision(gpu_arena_data(slot)),
            GpuCheck::Transform,
        );
        Transform { slot: slot }
    }
    pub fn from_matrices(mut world: Mat3x4, mut normal: Mat3x3) -> Self {
        let slot = gpu_api_object(true, GpuObjectKind::Transform);
        gpu_api_checked(
            gpu_scene_model_matrices(
                gpu_arena_data(slot),
                view_of(&mut world),
                view_of(&mut normal),
            ),
            GpuCheck::Transform,
        );
        gpu_api_checked(
            gpu_model_revision(gpu_arena_data(slot)),
            GpuCheck::Transform,
        );
        Transform { slot: slot }
    }

    pub fn world(&self) -> Mat3x4 {
        gpu_api_model_world(gpu_arena_data(self.slot))
    }
    pub fn normal(&self) -> Mat3x3 {
        gpu_api_model_normal(gpu_arena_data(self.slot))
    }

    pub fn set_pose(&mut self, pose: Pose) {
        gpu_api_model_set_pose(gpu_arena_data(self.slot), pose)
    }

    pub fn set_matrices(&mut self, world: Mat3x4, normal: Mat3x3) {
        gpu_api_model_set_matrices(gpu_arena_data(self.slot), world, normal)
    }

    pub fn ptr(&self) -> TransformPtr {
        TransformPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for Transform {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, true, GpuObjectKind::Transform) {
            gpu_api_fail(
                GpuFault::ObjectRelease,
                GpuObjectKind::Transform as u16,
                self.slot,
            );
        }
    }
}

#[rcc_resource]
pub struct Material {
    slot: u16,
}
impl Material {
    pub fn new(desc: MaterialDesc) -> Self {
        let slot = gpu_api_object(false, GpuObjectKind::Material);
        gpu_api_checked(
            gpu_scene_material_init(gpu_arena_data(slot), desc),
            GpuCheck::Material,
        );
        Material { slot: slot }
    }

    pub fn set_uniform_color(&mut self, color: Rgba8) {
        gpu_api_material_set_uniform_color(gpu_arena_data(self.slot), color)
    }

    pub fn set_specular(&mut self, color: Rgb8, shininess: Shininess) {
        gpu_api_material_set_specular(gpu_arena_data(self.slot), color, shininess)
    }

    pub fn set_response(&mut self, ambient: fix32, directional: fix32) {
        gpu_api_material_set_response(gpu_arena_data(self.slot), ambient, directional)
    }

    pub fn set_unlit(&mut self, unlit: bool) {
        gpu_api_material_set_unlit(gpu_arena_data(self.slot), unlit)
    }

    pub fn ptr(&self) -> MaterialPtr {
        MaterialPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for Material {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GpuObjectKind::Material) {
            gpu_api_fail(
                GpuFault::ObjectRelease,
                GpuObjectKind::Material as u16,
                self.slot,
            );
        }
    }
}

#[rcc_resource]
pub struct RenderState {
    slot: u16,
}
impl RenderState {
    pub fn new(desc: RenderStateDesc) -> Self {
        let slot = gpu_api_object(false, GpuObjectKind::State);
        gpu_api_checked(
            gpu_scene_state_init(gpu_arena_data(slot), desc),
            GpuCheck::State,
        );
        RenderState { slot: slot }
    }

    pub fn set_depth(&mut self, func: DepthFunc, write: bool) {
        gpu_api_render_state_set_depth(gpu_arena_data(self.slot), func, write)
    }

    pub fn set_blend(&mut self, mode: BlendMode) {
        gpu_api_render_state_set_blend(gpu_arena_data(self.slot), mode)
    }

    pub fn set_filter(&mut self, filter: TextureFilter) {
        gpu_api_render_state_set_filter(gpu_arena_data(self.slot), filter)
    }

    pub fn set_lighting(&mut self, light: Lighting) {
        gpu_api_render_state_set_lighting(gpu_arena_data(self.slot), light)
    }

    pub fn ptr(&self) -> RenderStatePtr {
        RenderStatePtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for RenderState {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GpuObjectKind::State) {
            gpu_api_fail(
                GpuFault::ObjectRelease,
                GpuObjectKind::State as u16,
                self.slot,
            );
        }
    }
}

#[rcc_resource]
pub struct Texture {
    slot: u16,
}
impl Texture {
    pub fn from_native(desc: TextureDesc) -> Self {
        let slot = gpu_api_object(false, GpuObjectKind::Texture);
        gpu_api_checked(
            gpu_scene_texture_init(gpu_arena_data(slot), desc),
            GpuCheck::Texture,
        );
        Texture { slot: slot }
    }

    pub fn view(&self) -> TextureView {
        let view = TextureView {
            words: Buf::new([0; 6]),
        };
        gpu_scene_texture_view(view.data(), gpu_arena_data(self.slot));
        view
    }

    pub fn ptr(&self) -> TexturePtr {
        TexturePtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GpuObjectKind::Texture) {
            gpu_api_fail(
                GpuFault::ObjectRelease,
                GpuObjectKind::Texture as u16,
                self.slot,
            );
        }
    }
}

#[derive(Copy, Clone)]
pub struct UiAtlasDesc {
    pub memory: GpuMemory,
    pub glyph_count: u16,
}
impl UiAtlasDesc {
    /// Caller guarantees immutable, exclusive backing storage for the atlas.
    /// Native records occupy 256 bytes; their base is 64-KiB aligned.
    pub const unsafe fn from_asset(asset: AssetBytes) -> Self {
        debug_assert!(asset.first_sector & 511 == 0);
        debug_assert!(asset.byte_len_hi == 0);
        debug_assert!(asset.byte_len_lo != 0 && asset.byte_len_lo & 255 == 0);
        Self {
            memory: unsafe { GpuMemory::from_asset(asset) },
            glyph_count: asset.byte_len_lo / 256,
        }
    }
}

#[rcc_resource]
pub struct UiAtlas {
    slot: u16,
}
impl UiAtlas {
    /// Caller owns the immutable external allocation for the atlas's lifetime.
    pub fn from_desc(desc: UiAtlasDesc) -> Self {
        Self::from_native(desc.memory, desc.glyph_count)
    }
    pub fn from_native(memory: GpuMemory, glyph_count: u16) -> Self {
        let slot = gpu_api_object(false, GpuObjectKind::UiAtlas);
        gpu_api_checked(
            gpu_ui_atlas_init(gpu_arena_data(slot), memory, glyph_count),
            GpuCheck::UiAtlas,
        );
        UiAtlas { slot: slot }
    }

    pub fn ptr(&self) -> UiAtlasPtr {
        UiAtlasPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for UiAtlas {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GpuObjectKind::UiAtlas) {
            gpu_api_fail(
                GpuFault::ObjectRelease,
                GpuObjectKind::UiAtlas as u16,
                self.slot,
            );
        }
    }
}

#[rcc_resource]
pub struct UiGrid {
    slot: u16,
}
impl UiGrid {
    pub fn from_storage(storage: UiGridStorage, atlas: &UiAtlas) -> Self {
        Self::from_memory(GpuMemory::from_buffer(storage.storage), atlas)
    }
    pub fn clear(&mut self) {
        let ptr = self.ptr();
        ptr.clear()
    }
    pub fn from_memory(memory: GpuMemory, atlas: &UiAtlas) -> Self {
        let slot = gpu_api_object(false, GpuObjectKind::UiGrid);
        gpu_api_checked(
            gpu_ui_grid_init(gpu_arena_data(slot), memory, gpu_arena_data(atlas.slot)),
            GpuCheck::UiGrid,
        );
        UiGrid { slot: slot }
    }

    pub fn fill(&mut self, glyph: u8) {
        gpu_api_ui_fill(gpu_arena_data(self.slot), glyph)
    }

    /// Write one row without repeating its length in a CellRect.
    pub fn write_text(&mut self, x: u16, y: u16, glyphs: &[u8]) {
        self.write(
            CellRect {
                x: x,
                y: y,
                width: glyphs.len() as u16,
                height: 1,
            },
            glyphs,
        );
    }
    /// Replace exactly one cell, preserving the adjacent packed byte.
    pub fn write_glyph(&mut self, x: u16, y: u16, glyph: u8) {
        gpu_api_ui_write_glyph(gpu_arena_data(self.slot), x, y, glyph)
    }
    pub fn write(&mut self, rect: CellRect, glyphs: &[u8]) {
        gpu_api_ui_write(gpu_arena_data(self.slot), rect, glyphs)
    }

    pub fn ptr(&self) -> UiGridPtr {
        UiGridPtr {
            handle: gpu_arena_handle(self.slot),
        }
    }
    pub fn release(self) {
        drop(self);
    }
}
impl Drop for UiGrid {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GpuObjectKind::UiGrid) {
            gpu_api_fail(
                GpuFault::ObjectRelease,
                GpuObjectKind::UiGrid as u16,
                self.slot,
            );
        }
    }
}

/// A texture sampling description borrowed only during recording.
pub struct TextureView {
    words: Buf<u16, 6>,
}
impl TextureView {
    fn data(&self) -> Array<u16> {
        self.words.as_array()
    }

    pub fn set_address_mode(&mut self, mode: TextureAddressMode) {
        let mut data = self.data();
        gpu_api_checked(data[GPU_VIEW_FLAGS] & 2 == 0, GpuCheck::View);
        data[GPU_VIEW_FLAGS] = (data[GPU_VIEW_FLAGS] & 55) | ((mode as u16) << 3);
    }

    /// Limit the chain to a minimum side of 1, 2, 4 or 8 texels.
    /// ClampToEdge also clamps each selected grid cell independently.
    pub fn set_min_mip_size(&mut self, size_log2: u16) {
        let mut data = self.data();
        gpu_api_checked(
            size_log2 <= 3 && size_log2 <= data[GPU_VIEW_EXPONENT],
            GpuCheck::View,
        );
        gpu_api_checked(
            size_log2 == 0 || data[GPU_VIEW_FLAGS] & 3 == 1,
            GpuCheck::View,
        );
        data[GPU_VIEW_FLAGS] = (data[GPU_VIEW_FLAGS] & 15) | (size_log2 << 4);
    }

    /// Divide each axis into 1, 2, 4 or 8 cells and clamp within the UV's cell.
    /// Full mips are required for nonzero grids. UV endpoints must stay inside
    /// their intended cell; a boundary belongs to the next cell.
    pub fn set_atlas_grid(&mut self, grid_log2: u16) {
        self.set_min_mip_size(grid_log2);
        self.set_address_mode(TextureAddressMode::ClampToEdge);
    }

    pub fn width(&self) -> u16 {
        gpu_scene_view_width(self.data())
    }
    pub fn height(&self) -> u16 {
        gpu_scene_view_height(self.data())
    }
}

fn gpu_api_set_camera(slot: u16, camera: CameraPtr) {
    let camera_slot = gpu_arena_resolve(camera.handle, GpuObjectKind::Camera);
    let data = gpu_arena_data(camera_slot);
    gpu_api_checked(gpu_bind_desired(slot, true, data), GpuCheck::Camera);
}
fn gpu_api_set_state(slot: u16, state: RenderStatePtr) {
    gpu_api_checked(gpu_bind_desired(slot, false, state.data()), GpuCheck::State);
}
#[repr(u16)]
#[derive(Copy, Clone)]
pub enum BillboardFacing {
    Vertical,
    CameraPlane,
}

fn gpu_api_draw_textured(
    slot: u16,
    transform: TransformPtr,
    mesh: MeshPtr,
    material: MaterialPtr,
    view: &TextureView,
) {
    gpu_api_draw(
        slot,
        gpu_arena_resolve(transform.handle, GpuObjectKind::Transform),
        mesh,
        gpu_arena_resolve(material.handle, GpuObjectKind::Material),
        view.words.as_ptr(),
        0,
    );
}
fn gpu_api_draw_colored(slot: u16, transform: TransformPtr, mesh: MeshPtr, material: MaterialPtr) {
    gpu_api_draw(
        slot,
        gpu_arena_resolve(transform.handle, GpuObjectKind::Transform),
        mesh,
        gpu_arena_resolve(material.handle, GpuObjectKind::Material),
        Ptr::from_addr(0),
        0,
    );
}

impl Drop for PreparedPart {
    fn drop(&mut self) {
        gpu_group_abandon(self.slot);
    }
}
impl PreparedPart {
    /// Mark a range endpoint and return its unpadded instruction offset.
    /// A draw that follows the mark establishes its own World and material.
    pub fn mark_slice(&mut self) -> u16 {
        let mut record = gpu_record_data(self.slot);
        let first = record[GPU_RECORD_COMMAND_USED] >> 1;
        gpu_api_checked(
            gpu_group_mark_slice(record[GPU_GROUP_RECORDING], first),
            GpuCheck::Prepared,
        );
        #[cfg(gpu_debug)]
        {
            record[GPU_GROUP_SLICE_START] = first;
        }
        record[GPU_RECORD_VALID] &= 65535 ^ (2 | 4 | GPU_BIND_RESPONSE_VALID);
        first
    }

    /// Publish this part for use in later passes.
    pub fn finish(self) {
        gpu_api_checked(gpu_group_end(self.slot), GpuCheck::Prepared);
        resource_disarm(self);
    }
    pub fn set_state(&mut self, state: RenderStatePtr) {
        gpu_api_set_state(self.slot, state);
    }
    pub fn draw(
        &mut self,
        transform: TransformPtr,
        mesh: MeshPtr,
        material: MaterialPtr,
        view: &TextureView,
    ) {
        gpu_api_draw_textured(self.slot, transform, mesh, material, view);
    }
    pub fn draw_colored(&mut self, transform: TransformPtr, mesh: MeshPtr, material: MaterialPtr) {
        gpu_api_draw_colored(self.slot, transform, mesh, material);
    }
    pub fn draw_billboard(
        &mut self,
        transform: TransformPtr,
        mesh: MeshPtr,
        material: MaterialPtr,
        view: &TextureView,
        facing: BillboardFacing,
    ) {
        gpu_api_draw(
            self.slot,
            gpu_arena_resolve(transform.handle, GpuObjectKind::Transform),
            mesh,
            gpu_arena_resolve(material.handle, GpuObjectKind::Material),
            view.words.as_ptr(),
            facing as u16 + 1,
        );
    }
}
fn gpu_api_draw(
    slot: u16,
    transform_slot: u16,
    mesh_ptr: MeshPtr,
    material_slot: u16,
    view_address: Ptr,
    billboard: u16,
) {
    gpu_unlit_end(slot);
    let mesh = mesh_ptr.data();
    gpu_api_checked(
        mesh_ptr.first <= mesh[GPU_MESH_COUNT]
            && mesh_ptr.count <= mesh[GPU_MESH_COUNT] - mesh_ptr.first,
        GpuCheck::Mesh,
    );
    let textured = view_address.addr() != 0;
    if mesh_ptr.count == 0 {
        return;
    }
    let c = gpu_record_data(slot);
    let part = c[GPU_GROUP_RECORDING] != 0;
    let required = if part { 16u16 } else { 24u16 };
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16
        || c[GPU_RECORD_VALID] & required != required
    {
        gpu_api_fail(GpuFault::DrawBindings, slot, 0);
    }
    let mut facts: Buf<u16, 8> = Buf::new([
        mesh[GPU_MESH_STORAGE],
        mesh[GPU_MESH_STORAGE_COUNT],
        ResourceFactKind::Mesh as u16,
        GPU_FACT_READ,
        0,
        0,
        0,
        0,
    ]);
    let mut count: u16 = 1;
    let native = gpu_native_scratch();
    let mut view = native;
    if textured {
        view = unsafe { view_address.as_u16_array() };
        facts[4u16] = view[GPU_VIEW_BASE];
        facts[5u16] = view[GPU_VIEW_COUNT];
        facts[6u16] = ResourceFactKind::Texture as u16;
        facts[7u16] = GPU_FACT_READ;
        count = 2;
    }
    let fact_plan: Buf<u16, 2> = Buf::new([0; 2]);
    gpu_api_checked(
        gpu_record_resource_room(slot, facts.as_array(), count, fact_plan.as_array()),
        GpuCheck::Mesh,
    );
    let desired = unsafe { c.as_ptr().add(GPU_CB_CAMERA as i16).as_u16_array() };
    let material = gpu_arena_data(material_slot);
    if !part && !gpu_bind_scene_current(slot) {
        gpu_api_checked(gpu_bind_prepare_scene(slot, native), GpuCheck::Mesh);
    }
    let state = unsafe { c.as_ptr().add(GPU_CB_STATE as i16).as_u16_array() };
    #[cfg(gpu_debug)]
    {
        if part {
            gpu_api_checked(gpu_group_note_responses(slot, material), GpuCheck::Prepared);
        } else {
            gpu_api_checked(
                gpu_scene_responses_valid(
                    state,
                    gpu_scene_fix_read(material, GPU_MATERIAL_AMBIENT),
                    gpu_scene_fix_read(material, GPU_MATERIAL_DIRECTIONAL),
                ),
                GpuCheck::Mesh,
            );
        }
    }
    gpu_api_checked(
        gpu_scene_material_prepare(state, material, view, textured, native),
        GpuCheck::Mesh,
    );
    let transform = gpu_arena_data(transform_slot);
    let mut native = native;
    native[GPU_SCRATCH_BILLBOARD] = (billboard != 0) as u16;
    native[GPU_SCRATCH_WORLD_MODE] = 1;
    if billboard == BillboardFacing::Vertical as u16 + 1 {
        let mut native = native;
        native[GPU_SCRATCH_WORLD_MODE] = 3;
    }
    if !part && gpu_bind_view_changed(slot) {
        gpu_scene_view_prepare(desired, native);
    }
    if billboard != 0 {
        gpu_api_checked(material[GPU_MATERIAL_FLAGS] & 1 != 0, GpuCheck::Mesh);
    }
    gpu_api_checked(
        gpu_mesh_draw_range(
            slot,
            mesh,
            transform,
            native,
            mesh_ptr.first,
            mesh_ptr.count,
        ),
        GpuCheck::Mesh,
    );

    gpu_record_resource_commit(slot, facts.as_array(), count, fact_plan.as_array());
}

impl Shininess {
    pub fn nearest(exponent: u16) -> Self {
        gpu_scene_shininess_nearest(exponent)
    }
    pub fn exponent(&self) -> u16 {
        gpu_scene_shininess_exponent(*self)
    }
}

// Copy references do not own or release their referent. Keep its resource body alive.

#[derive(Copy, Clone)]
pub struct CameraPtr {
    handle: GpuObjectHandle,
}

impl CameraPtr {
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::Camera))
    }
    pub fn view(self) -> Mat3x4 {
        gpu_api_camera_view(self.data())
    }
    pub fn set_pose(self, eye: Vec3, angles: Vec3) {
        gpu_api_camera_set_pose(self.data(), eye, angles)
    }
    pub fn set_projection(self, projection: Projection) {
        gpu_api_camera_set_projection(self.data(), projection)
    }
    pub fn set_look_at(self, eye: Vec3, target: Vec3, up: Vec3) {
        gpu_api_camera_set_look_at(self.data(), eye, target, up)
    }
    pub fn set_look_at_y_up(self, eye: Vec3, target: Vec3) {
        gpu_api_camera_set_look_at_y_up(self.data(), eye, target)
    }
    pub fn set_look_to(self, eye: Vec3, direction: Vec3, up: Vec3) {
        gpu_api_camera_set_look_to(self.data(), eye, direction, up)
    }
    pub fn set_view(self, view: Mat3x4) {
        gpu_api_camera_set_view(self.data(), view)
    }
}

fn gpu_api_camera_view(data: Array<u16>) -> Mat3x4 {
    gpu_scene_camera_get_view(data)
}

fn gpu_api_camera_set_pose(data: Array<u16>, eye: Vec3, angles: Vec3) {
    gpu_api_checked(gpu_scene_camera_pose(data, eye, angles), GpuCheck::Camera);
    gpu_api_checked(gpu_camera_revision(data), GpuCheck::Camera);
}

fn gpu_api_camera_set_projection(data: Array<u16>, projection: Projection) {
    let focal = projection.focal_pixels();
    let changed =
        data[GPU_CAMERA_FOCAL] != focal.lo_bits() || data[GPU_CAMERA_FOCAL + 1] != focal.hi_bits();
    gpu_api_checked(
        gpu_scene_camera_projection(data, projection),
        GpuCheck::Camera,
    );
    if changed {
        gpu_api_checked(gpu_camera_revision(data), GpuCheck::Camera);
    }
}

fn gpu_api_camera_set_look_at(data: Array<u16>, eye: Vec3, target: Vec3, up: Vec3) {
    gpu_api_checked(
        gpu_scene_camera_look_at(data, eye, target, up),
        GpuCheck::Camera,
    );
    gpu_api_checked(gpu_camera_revision(data), GpuCheck::Camera);
}

fn gpu_api_camera_set_look_at_y_up(data: Array<u16>, eye: Vec3, target: Vec3) {
    gpu_api_checked(
        gpu_scene_camera_look_at_y_up(data, eye, target),
        GpuCheck::Camera,
    );
    gpu_api_checked(gpu_camera_revision(data), GpuCheck::Camera);
}

fn gpu_api_camera_set_look_to(data: Array<u16>, eye: Vec3, direction: Vec3, up: Vec3) {
    gpu_api_checked(
        gpu_scene_camera_look_to(data, eye, direction, up),
        GpuCheck::Camera,
    );
    gpu_api_checked(gpu_camera_revision(data), GpuCheck::Camera);
}

fn gpu_api_camera_set_view(data: Array<u16>, mut view: Mat3x4) {
    gpu_api_checked(
        gpu_scene_camera_view(data, view_of(&mut view)),
        GpuCheck::Camera,
    );
    gpu_api_checked(gpu_camera_revision(data), GpuCheck::Camera);
}

#[derive(Copy, Clone)]
pub struct ColorBufferPtr {
    handle: GpuObjectHandle,
}

#[derive(Copy, Clone)]
pub struct DepthBufferPtr {
    handle: GpuObjectHandle,
}

#[derive(Copy, Clone)]
pub struct MaterialPtr {
    handle: GpuObjectHandle,
}

impl MaterialPtr {
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::Material))
    }
    pub fn set_uniform_color(self, color: Rgba8) {
        gpu_api_material_set_uniform_color(self.data(), color)
    }
    pub fn set_specular(self, color: Rgb8, shininess: Shininess) {
        gpu_api_material_set_specular(self.data(), color, shininess)
    }
    pub fn set_response(self, ambient: fix32, directional: fix32) {
        gpu_api_material_set_response(self.data(), ambient, directional)
    }
    pub fn set_unlit(self, unlit: bool) {
        gpu_api_material_set_unlit(self.data(), unlit)
    }
}

fn gpu_api_material_set_uniform_color(data: Array<u16>, color: Rgba8) {
    gpu_api_checked(gpu_scene_material_color(data, color), GpuCheck::Material);
}

fn gpu_api_material_set_specular(data: Array<u16>, color: Rgb8, shininess: Shininess) {
    gpu_api_checked(
        gpu_scene_material_specular(data, color, shininess),
        GpuCheck::Material,
    );
}

fn gpu_api_material_set_response(data: Array<u16>, ambient: fix32, directional: fix32) {
    gpu_api_checked(
        gpu_scene_material_response(data, ambient, directional),
        GpuCheck::Material,
    );
}

fn gpu_api_material_set_unlit(data: Array<u16>, unlit: bool) {
    gpu_api_checked(gpu_scene_material_unlit(data, unlit), GpuCheck::Material);
}

#[derive(Copy, Clone)]
pub struct MeshPtr {
    handle: GpuObjectHandle,
    first: u16,
    count: u16,
}

impl MeshPtr {
    /// Select meshlets without allocating or copying a directory. Retains the
    /// same nonowning generation; nested slices are relative to this range.
    pub fn slice(self, first: u16, count: u16) -> Self {
        gpu_api_checked(
            first <= self.count && count <= self.count - first,
            GpuCheck::Mesh,
        );
        Self {
            handle: self.handle,
            first: self.first + first,
            count: count,
        }
    }
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::Mesh))
    }
    pub fn meshlet_count(self) -> u16 {
        let data = self.data();
        gpu_api_checked(
            self.first <= data[GPU_MESH_COUNT] && self.count <= data[GPU_MESH_COUNT] - self.first,
            GpuCheck::Mesh,
        );
        self.count
    }
}

#[derive(Copy, Clone)]
pub struct TransformPtr {
    handle: GpuObjectHandle,
}

impl TransformPtr {
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::Transform))
    }
    pub fn world(self) -> Mat3x4 {
        gpu_api_model_world(self.data())
    }
    pub fn normal(self) -> Mat3x3 {
        gpu_api_model_normal(self.data())
    }
    pub fn set_pose(self, pose: Pose) {
        gpu_api_model_set_pose(self.data(), pose)
    }
    pub fn set_matrices(self, world: Mat3x4, normal: Mat3x3) {
        gpu_api_model_set_matrices(self.data(), world, normal)
    }
}

fn gpu_api_model_world(data: Array<u16>) -> Mat3x4 {
    gpu_scene_model_world(data)
}

fn gpu_api_model_normal(data: Array<u16>) -> Mat3x3 {
    gpu_scene_model_normal(data)
}

fn gpu_api_model_set_pose(mut data: Array<u16>, pose: Pose) {
    gpu_api_checked(gpu_scene_model_init(data, pose), GpuCheck::Transform);
    gpu_api_checked(gpu_model_revision(data), GpuCheck::Transform);
}

fn gpu_api_model_set_matrices(mut data: Array<u16>, mut world: Mat3x4, mut normal: Mat3x3) {
    let world_words = unsafe { view_of(&mut world).as_ptr().as_u16_array() };
    let normal_words = unsafe { view_of(&mut normal).as_ptr().as_u16_array() };
    if gpu_bind_equal(data, GPU_MODEL_WORLD, world_words, 0, 24)
        && gpu_bind_equal(data, GPU_MODEL_NORMAL, normal_words, 0, 18)
    {
        return;
    }
    gpu_api_checked(
        gpu_scene_model_matrices(data, view_of(&mut world), view_of(&mut normal)),
        GpuCheck::Transform,
    );
    gpu_api_checked(gpu_model_revision(data), GpuCheck::Transform);
}

#[derive(Copy, Clone)]
pub struct RenderStatePtr {
    handle: GpuObjectHandle,
}

impl RenderStatePtr {
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::State))
    }
    pub fn set_depth(self, func: DepthFunc, write: bool) {
        gpu_api_render_state_set_depth(self.data(), func, write)
    }
    pub fn set_blend(self, mode: BlendMode) {
        gpu_api_render_state_set_blend(self.data(), mode)
    }
    pub fn set_filter(self, filter: TextureFilter) {
        gpu_api_render_state_set_filter(self.data(), filter)
    }
    pub fn set_lighting(self, light: Lighting) {
        gpu_api_render_state_set_lighting(self.data(), light)
    }
}

fn gpu_api_render_state_set_depth(data: Array<u16>, func: DepthFunc, write: bool) {
    gpu_api_checked(gpu_scene_state_depth(data, func, write), GpuCheck::State);
}

fn gpu_api_render_state_set_blend(data: Array<u16>, mode: BlendMode) {
    gpu_api_checked(gpu_scene_state_blend(data, mode), GpuCheck::State);
}

fn gpu_api_render_state_set_filter(data: Array<u16>, filter: TextureFilter) {
    gpu_api_checked(gpu_scene_state_filter(data, filter), GpuCheck::State);
}

fn gpu_api_render_state_set_lighting(data: Array<u16>, light: Lighting) {
    gpu_api_checked(gpu_scene_state_lighting(data, light), GpuCheck::State);
}

#[derive(Copy, Clone)]
pub struct TexturePtr {
    handle: GpuObjectHandle,
}

#[derive(Copy, Clone)]
pub struct UiAtlasPtr {
    handle: GpuObjectHandle,
}

#[derive(Copy, Clone)]
pub struct UiGridPtr {
    handle: GpuObjectHandle,
}

impl UiGridPtr {
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::UiGrid))
    }
    pub fn clear(self) {
        self.fill(255);
    }
    pub fn fill(self, glyph: u8) {
        gpu_api_ui_fill(self.data(), glyph)
    }
    /// Write one row without repeating its length in a CellRect.
    pub fn write_text(self, x: u16, y: u16, glyphs: &[u8]) {
        self.write(
            CellRect {
                x: x,
                y: y,
                width: glyphs.len() as u16,
                height: 1,
            },
            glyphs,
        );
    }
    /// Replace exactly one cell, preserving the adjacent packed byte.
    pub fn write_glyph(self, x: u16, y: u16, glyph: u8) {
        gpu_api_ui_write_glyph(self.data(), x, y, glyph)
    }
    pub fn write(self, rect: CellRect, glyphs: &[u8]) {
        gpu_api_ui_write(self.data(), rect, glyphs)
    }
}

fn gpu_api_ui_fill(data: Array<u16>, glyph: u8) {
    gpu_api_checked(gpu_ui_grid_fill(data, glyph), GpuCheck::UiGrid);
}

fn gpu_api_ui_write(data: Array<u16>, rect: CellRect, glyphs: &[u8]) {
    gpu_api_checked(gpu_ui_grid_write(data, rect, glyphs), GpuCheck::UiGrid);
}

fn gpu_api_ui_write_glyph(data: Array<u16>, x: u16, y: u16, glyph: u8) {
    gpu_api_checked(gpu_ui_grid_write_glyph(data, x, y, glyph), GpuCheck::UiGrid);
}

impl ColorBufferPtr {
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::Color))
    }
}

impl DepthBufferPtr {
    fn data(self) -> Array<u16> {
        gpu_arena_data(gpu_arena_resolve(self.handle, GpuObjectKind::Depth))
    }
}

/// Immutable directory and prevalidated unlit material bindings.
/// Keep the referenced GPU mesh and texture bytes resident through retirement.
/// A bank uses one lighting environment and one Transform snapshot per recording.
#[rcc_resource]
pub struct UnlitBank {
    data: u16,
}
impl UnlitBank {
    pub fn new(mesh: MeshPtr) -> Self {
        Self {
            data: gpu_unlit_new(mesh.data(), mesh.first, mesh.count).addr(),
        }
    }
    /// Register a fixed textured draw before the first preparation.
    pub fn add(
        &mut self,
        state: RenderStatePtr,
        material: MaterialPtr,
        view: &TextureView,
        vertical_billboard: bool,
    ) -> u16 {
        let mut flags: u16 = 1;
        if vertical_billboard {
            flags |= BANK_VERTICAL;
        }
        gpu_unlit_add(
            unsafe { Ptr::from_addr(self.data).as_u16_array() },
            state.data(),
            material.data(),
            view.data(),
            flags,
        )
    }
    pub fn add_colored(&mut self, state: RenderStatePtr, material: MaterialPtr) -> u16 {
        let data = unsafe { Ptr::from_addr(self.data).as_u16_array() };
        gpu_unlit_add(data, state.data(), material.data(), data, 0)
    }
}
impl Drop for UnlitBank {
    fn drop(&mut self) {
        if GPU_UNLIT_OWNER[0u16] == self.data {
            gpu_unlit_end(0);
        }
        if GPU_UNLIT_OWNER[1u16] == self.data {
            gpu_unlit_end(1);
        }
        free(Ptr::from_addr(self.data));
    }
}
