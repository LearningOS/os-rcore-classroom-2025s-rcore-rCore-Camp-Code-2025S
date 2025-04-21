//! Implementation of [`MapArea`] and [`MemorySet`].
use super::{ frame_alloc, FrameTracker };
use super::{ PTEFlags, PageTable, PageTableEntry };
use super::{ PhysAddr, PhysPageNum, VirtAddr, VirtPageNum };
use super::{ StepByOne, VPNRange };
#[allow(unused_imports)]
use crate::config::{
    KERNEL_STACK_SIZE,
    MEMORY_END,
    PAGE_SIZE,
    TRAMPOLINE,
    TRAP_CONTEXT_BASE,
    USER_STACK_SIZE,
};
use crate::sync::UPSafeCell;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use lazy_static::*;
use riscv::register::satp;

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// The kernel's initial memory mapping(kernel address space)
    pub static ref KERNEL_SPACE: Arc<UPSafeCell<MemorySet>> = Arc::new(unsafe {
        UPSafeCell::new(MemorySet::new_kernel())
    });
}
/// address space
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>, //逻辑段数组
}

impl MemorySet {
    /// Create a new empty `MemorySet`.
    pub fn new_bare() -> Self {
        // 创建一个空的地址空间
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(), //无内存区域
        }
    }
    /// Get the page table token
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    /// Assume that no conflicts.
    pub fn insert_framed_area(
        //为进程添加一段动态分配的虚拟内存
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission
    ) {
        self.push(MapArea::new(start_va, end_va, MapType::Framed, permission), None);
    }
    /// remove a area
    pub fn remove_area_with_start_vpn(&mut self, start_vpn: VirtPageNum) {
        if let Some((idx, area)) = self
            .areas
            .iter_mut()
            .enumerate()
            .find(|(_, area)| area.vpn_range.get_start() == start_vpn)
        {
            area.unmap(&mut self.page_table);
            self.areas.remove(idx);
        }
    }
    /// Add a new MapArea into this MemorySet.
    /// Assuming that there are no conflicts in the virtual address
    /// space.
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        //在当前地址空间插入一个新的逻辑段 map_area
        map_area.map(&mut self.page_table); //建立从虚拟页到物理页的映射
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        self.areas.push(map_area);
    }
    /// Mention that trampoline is not collected by areas.
    fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X
        );
    }
    /// Without kernel stacks.
    pub fn new_kernel() -> Self {
        //初始化内核的地址空间 建立数据,物理内存的映射
        let mut memory_set = Self::new_bare(); //位于虚拟地址顶端的跳d板
        // map trampoline
        memory_set.map_trampoline();
        // map kernel sections
        info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        info!(".bss [{:#x}, {:#x})", sbss_with_stack as usize, ebss as usize);
        info!("mapping .text section");
        //对.text字段初始化 接下来一共5个逻辑段
        memory_set.push(
            MapArea::new(
                (stext as usize).into(), // start VA
                (etext as usize).into(), //end VA
                MapType::Identical, // VA=PA
                MapPermission::R | MapPermission::X //Read & EXcute
            ),
            None
        );
        info!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R
            ),
            None
        );
        info!("mapping .data section");
        memory_set.push(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W
            ),
            None
        );
        info!("mapping .bss section");
        memory_set.push(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W
            ),
            None
        );
        info!("mapping physical memory");
        //物理内存
        memory_set.push(
            MapArea::new(
                (ekernel as usize).into(), // 内核结束地址
                MEMORY_END.into(), //物理内存末尾
                MapType::Identical,
                MapPermission::R | MapPermission::W
            ),
            None
        );
        memory_set
    }
    /// Include sections in elf and trampoline and TrapContext and user stack,
    /// also returns user_sp_base and entry point.
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        //从 ELF 文件加载用户程序
        let mut memory_set = Self::new_bare(); //空PageTable
        // map trampoline
        memory_set.map_trampoline(); //跳板
        // map program headers of elf, with U flag
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!"); //校验文件格式合法性
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        //映射 ELF 的加载段
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            //确认program header的类型是LOAD
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                // 获取虚拟地址范围start_va ,end_va 和 权限
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U; //用户态
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                // 创建映射区域并拷贝数据
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.vpn_range.get_end();
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize])
                );
            }
        }
        // map user stack with U flags
        // 处理用户栈
        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_bottom: usize = max_end_va.into(); //栈底
        // guard page
        user_stack_bottom += PAGE_SIZE;
        let user_stack_top = user_stack_bottom + USER_STACK_SIZE; //栈顶
        memory_set.push(
            MapArea::new(
                user_stack_bottom.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U
            ),
            None
        );
        // used in sbrk
        memory_set.push(
            MapArea::new(
                user_stack_top.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U
            ),
            None
        );
        // map TrapContext
        memory_set.push(
            MapArea::new(
                TRAP_CONTEXT_BASE.into(),
                TRAMPOLINE.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W
            ),
            None
        );
        (memory_set, user_stack_top, elf.header.pt2.entry_point() as usize)
    }
    /// Create a new address space by copy code&data from a exited process's address space.
    /// 复制已经有的用户地址空间 物理内存独立但内容相同
    pub fn from_existed_user(user_space: &Self) -> Self {
        let mut memory_set = Self::new_bare();//创建新的地址空间
        // map trampoline
        memory_set.map_trampoline(); //映射跳板页
        // copy data sections/trap_context/user_stack
        for area in user_space.areas.iter() {
            let new_area = MapArea::from_another(area);
            memory_set.push(new_area, None); // 插入新地址空间（分配物理页）
            // copy data from another space
            // 逐页复制数据
            for vpn in area.vpn_range {
                let src_ppn = user_space.translate(vpn).unwrap().ppn();  //原来的物理页面
                let dst_ppn = memory_set.translate(vpn).unwrap().ppn();  // 新物理页
                dst_ppn
                    .get_bytes_array()
                    .copy_from_slice(src_ppn.get_bytes_array());
            }
        }
        memory_set
    }
    /// Change page table by writing satp CSR Register.
    pub fn activate(&self) {
        let satp = self.page_table.token(); //生成stap
        unsafe {
            satp::write(satp); //写入satp寄存器
            asm!("sfence.vma"); //刷新TLB
        }
    }
    /// Translate a virtual page number to a page table entry
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }

    ///Remove all `MapArea`
    pub fn recycle_data_pages(&mut self) {
        self.areas.clear();
    }

    /// shrink the area to new_end
    #[allow(unused)]
    pub fn shrink_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if
            let Some(area) = self.areas
                .iter_mut()
                .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.shrink_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    /// append the area to new_end
    #[allow(unused)]
    pub fn append_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if
            let Some(area) = self.areas
                .iter_mut()
                .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.append_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }
    /// memset the area with the given flag
    #[allow(unused)]
    pub fn mmap(&mut self, _start: VirtPageNum, _end: VirtPageNum, _falg: MapPermission) {
        let mut area = MapArea::new(_start.into(), _end.into(), MapType::Framed, _falg);
        area.map(&mut self.page_table);
        self.areas.push(area);
    }
    /// 查找并取消map
    pub fn munmap(&mut self, start_vpn: VirtPageNum, end_vpn: VirtPageNum) -> isize {
        self.areas
            .iter_mut()
            .enumerate()
            .find_map(|(i, area)| {
                if area.vpn_range.get_start() == start_vpn && area.vpn_range.get_end() == end_vpn {
                    Some(i)
                } else {
                    None
                }
            })
            .map(|i| {
                self.areas[i].unmap(&mut self.page_table);
                self.areas.remove(i);
                0
            })
            .unwrap_or(-1)
    }
    /// 检查self的area的range范围和_start _end之间的关系
    pub fn is_overlap(&self, _start: VirtPageNum, _end: VirtPageNum) -> bool {
        /*
        [_start,_end)
        [get_start,get_end)
        如果重叠:
        两个区间[A, B)和[C, D)重叠的条件是:

        A < D(第一个区间的起点在第二个区间结束前)
        B > C(第一个区间的终点在第二个区间开始后)
        
        */
        for area in self.areas.iter() {
            if area.vpn_range.get_start() < _end && area.vpn_range.get_end() > _start {
                return true;
            }
        }
        return false;
    }
}
/// map area structure, controls a contiguous piece of virtual memory
pub struct MapArea {
    vpn_range: VPNRange, // [start,end) 虚拟页号范围的左闭右开区间,判断某个虚拟地址是否属于该区间
    data_frames: BTreeMap<VirtPageNum, FrameTracker>, //维护虚拟页号到FrameTracker的map关系
    map_type: MapType, //map的方式 恒等映射 or 动态分配,如果是恒等map,那么不需要data_frames
    map_perm: MapPermission, //访问权限 map_permission R W X U
}

impl MapArea {
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr, //end向上整
        map_type: MapType,
        map_perm: MapPermission
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor(); //start向下整
        let end_vpn: VirtPageNum = end_va.ceil(); //end向上整
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
    //复制一个逻辑段
    pub fn from_another(another: &Self) -> Self {
        Self {
            vpn_range: VPNRange::new(
                another.vpn_range.get_start(),
                 another.vpn_range.get_end()
            ),
            data_frames: BTreeMap::new(),
            map_type: another.map_type,
            map_perm: another.map_perm,
        }
    }
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;
        match self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0); // Identical是恒等映射 物理页号 = 虚拟页号
            }
            MapType::Framed => {
                let frame = frame_alloc().unwrap(); //动态分配
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        page_table.map(vpn, ppn, pte_flags);
    }
    //取消单页映射
    #[allow(unused)]
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        //仅 Framed 类型需释放物理页
        if self.map_type == MapType::Framed {
            self.data_frames.remove(&vpn); // 从 BTreeMap 移除并触发 FrameTracker 的 Drop
        }
        page_table.unmap(vpn); // 清除页表项//
    }

    //遍历逻辑段中的所有虚拟页面 map和unmap,传入数据和删除
    #[allow(unused)]
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn);
        }
    }
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn);
        }
    }
    #[allow(unused)]
    pub fn shrink_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(new_end, self.vpn_range.get_end()) {
            self.unmap_one(page_table, vpn);
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    #[allow(unused)]
    pub fn append_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(self.vpn_range.get_end(), new_end) {
            self.map_one(page_table, vpn);
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    /// data: start-aligned but maybe with shorter length
    /// assume that all frames were cleared before
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        //将数据(如 ELF 文件的代码段)拷贝到已映射的物理页中。
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table.translate(current_vpn).unwrap().ppn().get_bytes_array()
                [..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
/// map type for memory set: identical or framed
pub enum MapType {
    Identical, // 恒等映射(虚拟地址 = 物理地址)
    Framed, // 动态分配(虚拟地址与物理地址无关)
}

bitflags! {
    /// map permission corresponding to that in pte: `R W X U`
    pub struct MapPermission: u8 {
        ///Readable
        const R = 1 << 1;
        ///Writable
        const W = 1 << 2;
        ///Excutable
        const X = 1 << 3;
        ///Accessible in U mode
        ///User模块
        const U = 1 << 4;
    }
}

/// remap test in kernel space
#[allow(unused)]
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.exclusive_access();
    let mid_text: VirtAddr = (((stext as usize) + (etext as usize)) / 2).into();
    let mid_rodata: VirtAddr = (((srodata as usize) + (erodata as usize)) / 2).into();
    let mid_data: VirtAddr = (((sdata as usize) + (edata as usize)) / 2).into();
    assert!(!kernel_space.page_table.translate(mid_text.floor()).unwrap().writable());
    assert!(!kernel_space.page_table.translate(mid_rodata.floor()).unwrap().writable());
    assert!(!kernel_space.page_table.translate(mid_data.floor()).unwrap().executable());
    println!("remap_test passed!");
}
