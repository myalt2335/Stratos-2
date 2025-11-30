use core::alloc::{Layout, GlobalAlloc};
use core::mem::MaybeUninit;
use core::ptr::{addr_of_mut, null_mut, NonNull};
use bootloader_api::info::{BootInfo, MemoryRegionKind};
use crate::console;
use linked_list_allocator::{Heap, LockedHeap};
use spin::Mutex;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub const HEAP_SIZE: usize = 256 * 1024;
static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

pub unsafe fn init_heap() {
    let heap_ptr = addr_of_mut!(HEAP) as *mut u8;
    ALLOCATOR.lock().init(heap_ptr, HEAP_SIZE);
}

#[derive(Copy, Clone, Default)]
pub struct HeapStats {
    pub used: usize,
    pub free: usize,
    pub total: usize,
    pub peak_used: usize,
    pub alloc_count: usize,
    pub dealloc_count: usize,
}

static KHEAP_COUNTERS: Mutex<HeapStats> = Mutex::new(HeapStats {
    used: 0,
    free: 0,
    total: 0,
    peak_used: 0,
    alloc_count: 0,
    dealloc_count: 0,
});

pub unsafe fn kalloc(size: usize, align: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, align).unwrap();
    let ptr = ALLOCATOR.alloc(layout);
    if !ptr.is_null() {
        let mut c = KHEAP_COUNTERS.lock();
        c.used += size;
        c.free = c.free.saturating_sub(size);
        c.total = HEAP_SIZE;
        c.alloc_count += 1;
        if c.used > c.peak_used {
            c.peak_used = c.used;
        }
    }
    ptr
}

pub unsafe fn kdealloc(ptr: *mut u8, size: usize, align: usize) {
    let layout = Layout::from_size_align(size, align).unwrap();
    ALLOCATOR.dealloc(ptr, layout);
    let mut c = KHEAP_COUNTERS.lock();
    c.used = c.used.saturating_sub(size);
    c.free = c.free.saturating_add(size);
    c.total = HEAP_SIZE;
    c.dealloc_count += 1;
}

pub fn heap_stats() -> HeapStats {
    let allocator = ALLOCATOR.lock();
    let used = allocator.used();
    let free = allocator.free();
    let total = used + free;
    let mut c = KHEAP_COUNTERS.lock();
    c.used = used;
    c.free = free;
    c.total = total;
    *c
}

#[derive(Copy, Clone, Default)]
pub struct SystemStats {
    pub reserved: usize,
    pub free: usize,
    pub total: usize,
}

static mut TOTAL_RAM: usize = 0;

pub fn system_stats() -> SystemStats {
    let total = get_total_ram();
    // Reserved tracks memory the OS takes exclusively (kernel heap, static data, etc.).
    // The user arena is space meant to be given to apps, so we leave it out of "reserved".
    let reserved = HEAP_SIZE;
    let free = total.saturating_sub(reserved);
    SystemStats { reserved, free, total }
}

fn get_total_ram() -> usize {
    unsafe { TOTAL_RAM }
}

pub fn init_memory(boot_info: &BootInfo) {
    let total: usize = boot_info
        .memory_regions
        .iter()
        .filter(|r| r.kind == MemoryRegionKind::Usable)
        .map(|r| (r.end - r.start) as usize)
        .sum();
    unsafe { TOTAL_RAM = total; }
    unsafe { init_heap(); }
    init_user_arena();
}

pub type AppId = u32;

pub const USER_ARENA_SIZE: usize = 1024 * 1024;
pub const USER_ARENA_ALIGN: usize = 4096;
static mut USER_ARENA: MaybeUninit<[u8; USER_ARENA_SIZE]> = MaybeUninit::uninit();

const MAX_APPS: usize = 32;

#[derive(Copy, Clone, Default)]
pub struct AppHeapStats {
    pub used: usize,
    pub free: usize,
    pub total: usize,
    pub peak_used: usize,
    pub alloc_count: usize,
    pub dealloc_count: usize,
}

struct AppSlot {
    id: Option<AppId>,
    start: *mut u8,
    size: usize,
    offset: usize,
    heap: MaybeUninit<Heap>,
    stats: AppHeapStats,
    initialized: bool,
}

impl AppSlot {
    const fn new_uninit() -> Self {
        Self {
            id: None,
            start: null_mut(),
            size: 0,
            offset: 0,
            heap: MaybeUninit::uninit(),
            stats: AppHeapStats {
                used: 0,
                free: 0,
                total: 0,
                peak_used: 0,
                alloc_count: 0,
                dealloc_count: 0,
            },
            initialized: false,
        }
    }
    unsafe fn heap_mut(&mut self) -> &mut Heap {
        &mut *self.heap.as_mut_ptr()
    }
}

struct AppTable {
    arena_base: *mut u8,
    arena_size: usize,
    bump_offset: usize,
    free_regions: [(usize, usize); MAX_APPS],
    free_count: usize,
    slots: [AppSlot; MAX_APPS],
}

impl AppTable {
    const fn new_uninit() -> Self {
        const SLOT: AppSlot = AppSlot::new_uninit();
        Self {
            arena_base: null_mut(),
            arena_size: 0,
            bump_offset: 0,
            free_regions: [(0, 0); MAX_APPS],
            free_count: 0,
            slots: [SLOT; MAX_APPS],
        }
    }
    fn find_slot_by_id(&mut self, id: AppId) -> Option<&mut AppSlot> {
        self.slots.iter_mut().find(|s| s.id == Some(id) && s.initialized)
    }
    fn find_free_slot(&mut self) -> Option<&mut AppSlot> {
        self.slots.iter_mut().find(|s| !s.initialized)
    }
    fn alloc_region(&mut self, bytes: usize) -> Option<(*mut u8, usize, usize)> {
        for i in 0..self.free_count {
            let (off, sz) = self.free_regions[i];
            if sz >= bytes {
                let ptr = unsafe { self.arena_base.add(off) };
                self.free_count -= 1;
                self.free_regions[i] = self.free_regions[self.free_count];
                return Some((ptr, bytes, off));
            }
        }
        let align_mask = USER_ARENA_ALIGN - 1;
        let aligned_off = (self.bump_offset + align_mask) & !align_mask;
        let end = aligned_off.checked_add(bytes)?;
        if end > self.arena_size {
            return None;
        }
        let ptr = unsafe { self.arena_base.add(aligned_off) };
        self.bump_offset = end;
        Some((ptr, bytes, aligned_off))
    }
    fn free_region(&mut self, off: usize, size: usize) {
        if self.free_count < MAX_APPS {
            self.free_regions[self.free_count] = (off, size);
            self.free_count += 1;
        }
    }
    fn arena_free_for_new_regions(&self) -> usize {
        let mut free = self.arena_size.saturating_sub(self.bump_offset);
        for i in 0..self.free_count {
            free = free.saturating_add(self.free_regions[i].1);
        }
        free
    }
}

unsafe impl Send for AppTable {}
unsafe impl Sync for AppTable {}

static APPS: Mutex<AppTable> = Mutex::new(AppTable::new_uninit());

pub fn init_user_arena() {
    let base = addr_of_mut!(USER_ARENA) as *mut u8;
    let mut t = APPS.lock();
    t.arena_base = base;
    t.arena_size = USER_ARENA_SIZE;
    t.bump_offset = 0;
}

pub fn register_app(app_id: AppId, quota_bytes: usize) -> bool {
    let mut t = APPS.lock();
    if t.find_slot_by_id(app_id).is_some() {
        return true;
    }
    if t.arena_free_for_new_regions() < quota_bytes {
        return false;
    }
    let (region_ptr, region_size, region_off) = match t.alloc_region(quota_bytes) {
        Some(r) => r,
        None => return false,
    };
    let slot = match t.find_free_slot() {
        Some(s) => s,
        None => return false,
    };
    unsafe {
        let heap_ptr = slot.heap.as_mut_ptr();
        heap_ptr.write(Heap::empty());
        (*heap_ptr).init(region_ptr, region_size);
    }
    slot.id = Some(app_id);
    slot.start = region_ptr;
    slot.size = region_size;
    slot.offset = region_off;
    slot.stats = AppHeapStats {
        used: 0,
        free: region_size,
        total: region_size,
        peak_used: 0,
        alloc_count: 0,
        dealloc_count: 0,
    };
    slot.initialized = true;
    true
}

pub fn unregister_app(app_id: AppId) -> bool {
    let mut t = APPS.lock();
    let (off, sz) = {
        let slot = match t.find_slot_by_id(app_id) {
            Some(s) => s,
            None => return false,
        };
        let off = slot.offset;
        let sz = slot.size;
        slot.id = None;
        slot.start = null_mut();
        slot.size = 0;
        slot.offset = 0;
        slot.stats = AppHeapStats::default();
        slot.initialized = false;
        (off, sz)
    };
    t.free_region(off, sz);
    true
}

pub unsafe fn app_alloc(app_id: AppId, size: usize, align: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, align).unwrap();
    let mut t = APPS.lock();
    let Some(slot) = t.find_slot_by_id(app_id) else { return null_mut(); };
    if slot.stats.free < size {
        return null_mut();
    }
    match slot.heap_mut().allocate_first_fit(layout) {
        Ok(block) => {
            slot.stats.used += size;
            slot.stats.free = slot.size.saturating_sub(slot.stats.used);
            slot.stats.alloc_count += 1;
            if slot.stats.used > slot.stats.peak_used {
                slot.stats.peak_used = slot.stats.used;
            }
            block.as_ptr()
        }
        Err(_) => null_mut(),
    }
}

pub unsafe fn app_dealloc(app_id: AppId, ptr: *mut u8, size: usize, align: usize) -> bool {
    let layout = Layout::from_size_align(size, align).unwrap();
    let mut t = APPS.lock();
    let Some(slot) = t.find_slot_by_id(app_id) else { return false; };
    let begin = slot.start as usize;
    let end = begin + slot.size;
    let p = ptr as usize;
    if !(begin..end).contains(&p) {
        return false;
    }
    slot.heap_mut().deallocate(NonNull::new_unchecked(ptr), layout);
    slot.stats.used = slot.stats.used.saturating_sub(size);
    slot.stats.free = slot.size.saturating_sub(slot.stats.used);
    slot.stats.dealloc_count += 1;
    true
}

pub fn app_stats(app_id: AppId) -> Option<AppHeapStats> {
    let mut t = APPS.lock();
    let slot = t.find_slot_by_id(app_id)?;
    let free_now = unsafe { slot.heap_mut().free() };
    slot.stats.free = free_now;
    slot.stats.total = slot.size;
    slot.stats.used = slot.size.saturating_sub(free_now);
    Some(slot.stats)
}

#[allow(dead_code)]
pub fn app_can_reserve_now(app_id: AppId, bytes: usize) -> bool {
    app_stats(app_id).map(|s| s.free >= bytes).unwrap_or(false)
}

pub struct MemoryOverview {
    pub system: SystemStats,
    pub kernel_heap: HeapStats,
    pub user_arena_total: usize,
    pub user_arena_free_for_new_regions: usize,
    pub apps: [Option<(AppId, AppHeapStats)>; MAX_APPS],
    pub display: Option<console::DisplayBufferStats>,
}

pub fn memory_overview() -> MemoryOverview {
    let k = heap_stats();
    let sys = system_stats();
    let mut t = APPS.lock();
    let mut apps: [Option<(AppId, AppHeapStats)>; MAX_APPS] = [None; MAX_APPS];
    for (i, s) in t.slots.iter_mut().enumerate() {
        if s.initialized {
            let free_now = unsafe { s.heap_mut().free() };
            s.stats.free = free_now;
            s.stats.total = s.size;
            s.stats.used = s.size.saturating_sub(free_now);
            apps[i] = Some((s.id.unwrap_or(0), s.stats));
        }
    }
    MemoryOverview {
        system: sys,
        kernel_heap: k,
        user_arena_total: t.arena_size,
        user_arena_free_for_new_regions: t.arena_free_for_new_regions(),
        apps,
        display: console::display_buffer_stats(),
    }
}
