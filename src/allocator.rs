extern crate alloc;
use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::borrow::BorrowMut;
use core::cell::RefCell;
use core::cmp::max;
use core::fmt;
use core::mem::size_of;
use core::ops::DerefMut;
use core::ptr::null_mut;

use alloc::boxed::Box;

use crate::result::Result;
use crate::uefi::EfiMemoryDescriptor;
use crate::uefi::EfiMemoryType;
use crate::uefi::MemoryMapHolder;

// 1を引いた値の上位の0の数だけ右シフトして最も近い2のべき乗（1のビットが1つしかない数）を導く
// 最初に1を引かないと偶数を渡したときに1bitずれる
pub fn round_up_to_nearest_pow2(v: usize) -> Result<usize> {
    1usize
        .checked_shl(usize::BITS - v.wrapping_sub(1).leading_zeros())
        .ok_or("Out of range")
}

#[test_case]
fn round_up_to_nearest_pow2_test() {
    assert_eq!(round_up_to_nearest_pow2(0), Err("Out of range"));
    assert_eq!(round_up_to_nearest_pow2(1), Ok(1));
    assert_eq!(round_up_to_nearest_pow2(2), Ok(2));
    assert_eq!(round_up_to_nearest_pow2(3), Ok(4));
    assert_eq!(round_up_to_nearest_pow2(4), Ok(4));
    assert_eq!(round_up_to_nearest_pow2(5), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(6), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(7), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(8), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(9), Ok(16));
}

struct Header {
    next_header: Option<Box<Header>>,
    size: usize,
    is_allocated: bool,
    _reserved: usize,
}

const HEADER_SIZE: usize = size_of::<Header>();

#[allow(clippy::assertions_on_constants)]
const _: () = assert!(HEADER_SIZE == 32);
//  HEADER_SIZEは2の倍数になっている
const _: () = assert!(HEADER_SIZE.count_ones() == 1);
pub const LAYOUT_PAGE_4K: Layout = unsafe { Layout::from_size_align_unchecked(4096, 4096) };

impl Header {
    // 空き領域にメモリを確保するのに必要な十分なサイズがあるかどうかをチェックする
    // 最後に空きヘッダを追加するから * 2している
    fn can_provide(&self, size: usize, align: usize) -> bool {
        self.size >= size + HEADER_SIZE * 2 + align
    }
    fn is_allocated(&self) -> bool {
        self.is_allocated
    }
    // ヘッダの終わりのアドレス（生ポインタのアドレス値）
    fn end_addr(&self) -> usize {
        self as *const Header as usize + self.size
    }
    // Headerをaddrから作成する
    unsafe fn new_from_addr(addr: usize) -> Box<Header> {
        let header = addr as *mut Header;
        header.write(Header {
            next_header: None,
            size: 0,
            is_allocated: false,
            _reserved: 0,
        });
        Box::from_raw(addr as *mut Header)
    }
    // アドレスから確保済みのヘッダを生ポインタを使って返す
    unsafe fn from_allocated_regional(addr: *mut u8) -> Box<Header> {
        let header = addr.sub(HEADER_SIZE) as *mut Header;
        Box::from_raw(header)
    }
    // 指定されたサイズ・アラインメントでHeaderからメモリを切り出してみる
    fn provide(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        // 2のべき乗になるようにサイズを多めに確保する、最低でもHEADER_SIZE分（1つHeaderが作れるように）は確保されるように調整する
        let size = max(round_up_to_nearest_pow2(size).ok()?, HEADER_SIZE);
        let align = max(align, HEADER_SIZE);
        if self.is_allocated() || !self.can_provide(size, align) {
            None
        } else {
            // 今回のprovideを通じて消費したメモリの量
            let mut size_used = 0;
            // 提供するアドレスを先に決める、現在の最終アドレスからsize分だけ引いて
            let allocated_addr = (self.end_addr() - size) & !(align - 1);
            // アドレスからheaderを作る
            let mut header_for_allocated =
                unsafe { Self::new_from_addr(allocated_addr - HEADER_SIZE) };
            header_for_allocated.is_allocated = true;
            // 要求されたsizeとHEADER_SIZE分だけ確保したとする
            header_for_allocated.size = size + HEADER_SIZE;
            // 使用済みのsizeを更新する
            size_used += header_for_allocated.size;
            // 確保したヘッダの次の要素として確保した元のヘッダを置く
            // Before: 確保済み->確保元
            // After: 確保済み->確保済み->確保元
            header_for_allocated.next_header = self.next_header.take();
            // まだ確保元のヘッダに余裕がある場合
            if header_for_allocated.end_addr() != self.end_addr() {
                // 余剰のヘッダ
                let mut header_for_padding =
                  // 確保したヘッダの末尾から作る
                  unsafe { Self::new_from_addr(header_for_allocated.end_addr()) };
                header_for_padding.is_allocated = false;
                header_for_padding.size = self.end_addr() - header_for_allocated.end_addr();
                size_used += header_for_padding.size;
                // 確保済み->Padding->確保元
                header_for_padding.next_header = header_for_allocated.next_header.take();
                header_for_allocated.next_header = Some(header_for_padding);
            }
            assert!(self.size >= size_used + HEADER_SIZE);
            self.size -= size_used;
            // After: 確保済み->確保元->確保済み
            self.next_header = Some(header_for_allocated);
            Some(allocated_addr as *mut u8)
        }
    }
}

impl Drop for Header {
    fn drop(&mut self) {
        panic!("Header should not be dropped");
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Header @ {:#018X} {{size: {:#018X}, is_allocated: {} }}",
            self as *const Header as usize,
            self.size,
            self.is_allocated()
        )
    }
}

// アロケータ本体
pub struct FirstFitAllocator {
    first_header: RefCell<Option<Box<Header>>>,
}

#[global_allocator]
pub static ALLOCATOR: FirstFitAllocator = FirstFitAllocator {
    first_header: RefCell::new(None),
};

impl FirstFitAllocator {
    // allocが呼び出されたときに呼び出される
    pub fn alloc_with_options(&self, layout: Layout) -> *mut u8 {
        let mut header = self.first_header.borrow_mut();
        let mut header = header.deref_mut();
        // headerを順にたどって行く
        loop {
            match header {
                // 指定されたサイズで確保しようと試行する
                Some(e) => match e.provide(layout.size(), layout.align()) {
                    // 空き領域があればそれを返す
                    Some(p) => break p,
                    // 空き領域がなければ諦める
                    None => {
                        header = e.next_header.borrow_mut();
                        continue;
                    }
                },
                None => break null_mut::<u8>(),
            }
        }
    }

    // 空き領域をtreeに追加する
    fn add_free_from_descriptor(&self, desc: &EfiMemoryDescriptor) {
        let mut start_addr = desc.physical_start() as usize;
        // ページ数 * 4096で実際のメモリサイズを取得する
        let mut size = desc.number_of_pages() as usize * 4096;
        if start_addr == 0 {
            start_addr += 4096;
            // 1ページ分減らす
            size = size.saturating_sub(4096);
        }
        if size <= 4096 {
            return;
        }
        let mut header = unsafe { Header::new_from_addr(start_addr) };
        header.next_header = None;
        header.is_allocated = false;
        header.size = size;
        let mut first_header = self.first_header.borrow_mut();
        // replaceで置き換えて、元の値を得られる
        let prev_last = first_header.replace(header);
        drop(first_header);
        let mut header = self.first_header.borrow_mut();
        header.as_mut().unwrap().next_header = prev_last;
    }

    // uefiから渡されてきたmemory mapを元に初期化する
    pub fn init_with_mmap(&self, memory_map: &MemoryMapHolder) {
        for e in memory_map.iter() {
            if e.memory_type() != EfiMemoryType::CONVENTIONAL_MEMORY {
                continue;
            }
            self.add_free_from_descriptor(e);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use alloc::vec;

    #[test_case]
    fn malloc_iterate_free_and_malloc() {
        use alloc::vec::Vec;
        for i in 0..1000 {
            let mut vec = Vec::new();
            vec.resize(i, 10);
            // deallocateが呼ばれる（vecがdropするので）
        }
    }

    #[test_case]
    fn malloc_align() {
        let mut pointers = [null_mut::<u8>(); 100];
        for align in [1, 2, 4, 8, 16, 32, 4096] {
            for e in pointers.iter_mut() {
                *e = ALLOCATOR.alloc_with_options(
                    Layout::from_size_align(1234, align).expect("Failed to create Layout"),
                );
                assert!(*e as usize != 0);
                assert!((*e as usize) % align == 0);
            }
        }
    }

    #[test_case]
    fn allocated_objects_have_no_overlap() {
        let allocations = [
            Layout::from_size_align(128, 128).unwrap(),
            Layout::from_size_align(32, 32).unwrap(),
            Layout::from_size_align(8, 8).unwrap(),
            Layout::from_size_align(16, 16).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(4, 4).unwrap(),
            Layout::from_size_align(2, 2).unwrap(),
            Layout::from_size_align(600000, 64).unwrap(),
            Layout::from_size_align(64, 64).unwrap(),
            Layout::from_size_align(1, 1).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(3, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(600000, 64).unwrap(),
            Layout::from_size_align(6000, 64).unwrap(),
            Layout::from_size_align(60000, 64).unwrap(),
            Layout::from_size_align(60000, 64).unwrap(),
            Layout::from_size_align(60000, 64).unwrap(),
            Layout::from_size_align(60000, 64).unwrap(),
        ];
        let mut pointers = vec![null_mut::<u8>(); allocations.len()];
        for e in allocations.iter().zip(pointers.iter_mut()).enumerate() {
            let (i, (layout, pointer)) = e;
            *pointer = ALLOCATOR.alloc_with_options(*layout);
            // 確保した領域に書き込む
            for k in 0..layout.size() {
                unsafe {
                    *pointer.add(k) = i as u8;
                }
            }
        }
        for e in allocations.iter().zip(pointers.iter_mut()).enumerate() {
            let (i, (layout, pointer)) = e;
            // 書き込んだ領域が正しくアクセスできることを確認する
            for k in 0..layout.size() {
                assert!(unsafe { *pointer.add(k) } == i as u8);
            }
        }
        for e in allocations.iter().zip(pointers.iter_mut()).step_by(2) {
            let (layout, pointer) = e;
            // 全部freeする
            unsafe { ALLOCATOR.dealloc(*pointer, *layout) }
        }
        for e in allocations
            .iter()
            .zip(pointers.iter_mut())
            .enumerate()
            .skip(1)
            .step_by(2)
        {
            let (i, (layout, pointer)) = e;
            for k in 0..layout.size() {
                assert!(unsafe { *pointer.add(k) } == i as u8)
            }
        }
        for e in allocations
            .iter()
            .zip(pointers.iter_mut())
            .enumerate()
            .step_by(2)
        {
            let (i, (layout, pointer)) = e;
            *pointer = ALLOCATOR.alloc_with_options(*layout);
            for k in 0..layout.size() {
                // 生ポインタに書き込む
                unsafe { *pointer.add(k) = i as u8 }
            }
        }
        for e in allocations.iter().zip(pointers.iter_mut()).enumerate() {
            let (i, (layout, pointer)) = e;
            for k in 0..layout.size() {
                assert!(unsafe { *pointer.add(k) } == i as u8)
            }
        }
    }
    #[test_case]
    fn alloc_box() {
        const HANDLER_STACK_SIZE: usize = 64 * 1024;
        let b = Box::new([0u8; HANDLER_STACK_SIZE]);
        assert!(b.len() == HANDLER_STACK_SIZE)
    }
}

unsafe impl Sync for FirstFitAllocator {}

unsafe impl GlobalAlloc for FirstFitAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc_with_options(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let mut region = Header::from_allocated_regional(ptr);
        // 未確保にする
        region.is_allocated = false;
        Box::leak(region);
    }
}
