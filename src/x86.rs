extern crate alloc;

use alloc::boxed::Box;

use crate::error;
use crate::info;
use crate::result::Result;
use core::arch::asm;
use core::arch::global_asm;
use core::fmt;
use core::marker::PhantomData;
use core::mem::offset_of;
use core::mem::size_of;
use core::mem::size_of_val;
use core::pin::Pin;

pub fn hlt() {
    unsafe { asm!("hlt") }
}

pub fn busy_loop_hint() {
    unsafe { asm!("pause") }
}

pub fn read_io_port_u8(port: u16) -> u8 {
    let mut data: u8;
    unsafe {
        asm!(
          "in al, dx",
          out("al") data,
          in("dx") port
        )
    }
    data
}

pub fn write_io_port_u8(port: u16, data: u8) {
    unsafe {
        asm!("out dx, al",
        in("al") data,
        in("dx") port)
    }
}

pub type RootPageTable = [u8; 1024];

pub fn read_cr3() -> *mut PML4 {
    let mut cr3: *mut PML4;
    unsafe {
        asm!("mov rax, cr3",
              out("rax") cr3);
    }
    cr3
}

pub const PAGE_SIZE: usize = 4096;
const ATTR_MASK: u64 = 0xFFF;
const ATTR_PRESENT: u64 = 1 << 0;
const ATTR_WRITABLE: u64 = 1 << 1;
const ATTR_WRITE_THROUGH: u64 = 1 << 3;
const ATTR_CACHE_DISABLED: u64 = 1 << 4;

#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum PageAttr {
    NotPresent = 0,
    ReadWriteKernel = ATTR_PRESENT | ATTR_WRITABLE,
    ReadWriteIo = ATTR_PRESENT | ATTR_WRITABLE | ATTR_WRITE_THROUGH | ATTR_CACHE_DISABLED,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TranslationResult {
    PageMapped4K { phys: u64 },
    PageMapped2M { phys: u64 },
    PageMapped1G { phys: u64 },
}

// ここの値はCPUから得られた値をそのまま変換して得る
#[repr(transparent)]
pub struct Entry<const LEVEL: usize, const SHIFT: usize, NEXT> {
    value: u64,
    next_type: PhantomData<NEXT>,
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> Entry<LEVEL, SHIFT, NEXT> {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn is_present(&self) -> bool {
        (self.read_value() & (1 << 0)) != 0
    }
    fn is_writable(&self) -> bool {
        (self.read_value() & (1 << 2)) != 0
    }
    fn is_user(&self) -> bool {
        (self.read_value() & (1 << 2)) != 0
    }
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "L{}Entry @ {:#p} {{ {:#018X} {}{}{}",
            LEVEL,
            self,
            self.read_value(),
            if self.is_present() { "P" } else { "N" },
            if self.is_writable() { "W" } else { "R" },
            if self.is_user() { "U" } else { "S" },
        )?;
        write!(f, "}}")
    }
    fn table(&self) -> Result<&NEXT> {
        if self.is_present() {
            // 生ポインタでアクセスして、NEXT型に変換
            Ok(unsafe { &*((self.value & !ATTR_MASK) as *const NEXT) })
        } else {
            Err("Page Not Found")
        }
    }
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Display for Entry<LEVEL, SHIFT, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Debug for Entry<LEVEL, SHIFT, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format(f)
    }
}

#[repr(align(4096))]
pub struct Table<const LEVEL: usize, const SHIFT: usize, NEXT> {
    entry: [Entry<LEVEL, SHIFT, NEXT>; 512],
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> Table<LEVEL, SHIFT, NEXT> {
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "L{}Table @ {:#p} {{", LEVEL, self)?;
        for i in 0..512 {
            let e = &self.entry[i];
            if !e.is_present() {
                continue;
            }
            writeln!(f, " entry[{:3}] = {:?}", i, e)?;
        }
        writeln!(f, "}}")
    }
    pub fn next_level(&self, index: usize) -> Option<&NEXT> {
        self.entry.get(index).and_then(|e| e.table().ok())
    }
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Debug for Table<LEVEL, SHIFT, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format(f)
    }
}

pub type PT = Table<1, 12, [u8; PAGE_SIZE]>;
pub type PD = Table<2, 21, PT>;
pub type PDPT = Table<3, 30, PD>;
pub type PML4 = Table<4, 39, PDPT>;

// Code Segment
// movとかで直接変更すると壊れる
pub unsafe fn write_cs(cs: u16) {
    asm!(
        // raxにに保存する
        "lea rax, [rip + 2f]",
        "push cx",
        "push rax",
        // スタック: cx:raxにジャンプする→csレジスタが設定される
        "ljmp [rsp]",
        // ↓の命令を示すラベル
          "2:",
          // rspを+10=スタックポインタを10増やして開放する
          "add rsp, 8 + 2",
        in("cx") cs
    )
}

// Stack Segment
pub unsafe fn write_ss(selector: u16) {
    asm!(
      "mov ss, ax", in("ax") selector
    )
}

// その他のセグメントレジスタ
pub unsafe fn write_fs(selector: u16) {
    asm!(
      "mov fs, ax", in("ax") selector
    )
}

pub unsafe fn write_es(selector: u16) {
    asm!(
      "mov es, ax",
      in("ax") selector
    )
}

pub unsafe fn write_gs(selector: u16) {
    asm!(
      "mov gs, ax",
      in("ax") selector
    )
}

pub unsafe fn write_ds(selector: u16) {
    asm!(
      "mov ds, ax",
      in("ax") selector
    )
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct FPUContext {
    data: [u8; 512],
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Copy, Clone)]
struct GeneralRegisterContext {
    rax: u64,
    rdx: u64,
    rbx: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rcx: u64,
}
const _: () = assert!(size_of::<GeneralRegisterContext>() == (16 - 1) * 8);

#[allow(dead_code)]
#[repr(C)]
#[derive(Copy, Clone)]
struct InterruptContext {
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}
const _: () = assert!(size_of::<InterruptContext>() == 5 * 8);

#[allow(dead_code)]
#[repr(C)]
#[derive(Copy, Clone)]
struct InterruptInfo {
    fpu_context: FPUContext,
    _dummy: u64,
    greg: GeneralRegisterContext,
    error_code: u64,
    ctx: InterruptContext,
}
const _: () = assert!(size_of::<InterruptInfo>() == (16 + 4 + 1) * 8 + 8 + 512);

impl fmt::Debug for InterruptInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "
      {{
        rip: {:#018X}, CS: {:#06X},
        rsp: {:#018X}, SS: {:#06X},
        rbp: {:#018X},

        rflags: {:#018X},
        error_code: {:#018X},

        rax: {:#018X}, rcx: {:#018X},
        rdx: {:#018X}, rbx: {:#018X},
        rsi: {:#018X}, rdi: {:#018X},
        r8 : {:#018X}, r9 : {:#018X},
        r10: {:#018X}, r11: {:#018X},
        r12: {:#018X}, r13: {:#018X},
        r14: {:#018X}, r15: {:#018X},
      }}
        ",
            self.ctx.rip,
            self.ctx.cs,
            self.ctx.rsp,
            self.ctx.ss,
            self.greg.rbp,
            self.ctx.rflags,
            self.error_code,
            //
            self.greg.rax,
            self.greg.rcx,
            self.greg.rdx,
            self.greg.rbx,
            //
            self.greg.rsi,
            self.greg.rdi,
            //
            self.greg.r8,
            self.greg.r9,
            self.greg.r10,
            self.greg.r11,
            self.greg.r12,
            self.greg.r13,
            self.greg.r14,
            self.greg.r15,
        )
    }
}

// 割り込み時のエントリポイントを登録するアセンブリを生成するマクロ
macro_rules! interrupt_entrypoint {
    ($index:literal) => {
        global_asm!(concat!(
            ".global interrupt_entrypoint",
            stringify!($index),
            "\n",
            "interrupt_entrypoint",
            stringify!($index),
            ":\n",
            "push 0 \n",
            "push rcx \n",
            "mov rcx, ",
            stringify!($index),
            " \n",
            "jmp inthandler_common \n",
        ));
    };
}

macro_rules! interrupt_entrypoint_with_ecode {
    ($index:literal) => {
        global_asm!(concat!(
            ".global interrupt_entrypoint",
            stringify!($index),
            "\n",
            "interrupt_entrypoint",
            stringify!($index),
            ":\n",
            "push rcx \n", // rcxに割り込み番号が入っている
            "mov rcx, ",
            stringify!($index),
            " \n",
            "jmp inthandler_common \n",
        ));
    };
}

interrupt_entrypoint!(3);
interrupt_entrypoint!(6);
interrupt_entrypoint_with_ecode!(8);
interrupt_entrypoint_with_ecode!(13);
interrupt_entrypoint_with_ecode!(14);
interrupt_entrypoint!(32);

extern "sysv64" {
    fn interrupt_entrypoint3();
    fn interrupt_entrypoint6();
    fn interrupt_entrypoint8();
    fn interrupt_entrypoint13();
    fn interrupt_entrypoint14();
    fn interrupt_entrypoint32();
}

global_asm!(
    r#"
  .global inthandler_common
  inthandler_common:
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rbx
    push rdx
    push rax

    // fxsaveでsaveするためのスタック領域を確保
    // fxsaveで勝手に512バイト分上書きされるので事前に対比させている
    sub rsp, 512 + 8
    // FPU SIMDレジスタを保存
    fxsave64 [rsp]

    mov rdi, rsp
    mov rbp, rsp
    // 下位4bitを0にして16バイトアラインメントにする
    and rsp, -16
    // 割り込み番号をrcxに保存しているので、rsiにコピーする
    // inthandlerに引数として渡す
    mov rsi, rcx

    call inthandler

    // rbpに保存していたスタックポインタをrspに戻す
    mov rsp, rbp
    add rsp, 512 + 8

    pop rax
    pop rdx
    pop rbx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    pop rcx
    add rsp, 8
    // 割り込みから復帰
    iretq
  "#
);

// Page Fault
pub fn read_cr2() -> u64 {
    let mut cr2: u64;
    unsafe {
        asm!("mov rax, cr2",
              out("rax") cr2);
    }
    cr2
}

#[no_mangle]
extern "sysv64" fn inthandler(info: &InterruptInfo, index: usize) {
    error!("Intterupt Info: {:?}", info);
    error!("Exception {index:#04X}: ");
    match index {
        3 => {
            error!("Breakpoint");
        }
        6 => {
            error!("Invalid Opcode");
        }
        8 => {
            error!("Double Fault");
        }
        13 => {
            error!("General Protection Fault");
            // instruction pointer=次に実行する・実行中の命令のアドレス
            let rip = info.ctx.rip;
            error!("Bytes @ RIP({rip:#018X}):");
            let rip = rip as *const u8;
            let bytes = unsafe { core::slice::from_raw_parts(rip, 16) };
            error!(" ={bytes:02X?}");
        }
        14 => {
            error!("Page Fault");
            error!("CR2={:018X}", read_cr2());
            error!(
                "Caused by: A {} mode {} on a {} page, page structures are {}",
                // https://wiki.osdev.org/Exceptions#Error_code
                if info.error_code & 0b0000_0100 != 0 {
                    "user"
                } else {
                    "supervisor"
                },
                if info.error_code & 0b0001_0000 != 0 {
                    "instruction fetch"
                } else if info.error_code & 0b0010 != 0 {
                    "data write"
                } else {
                    "data read"
                },
                if info.error_code & 0b0001 != 0 {
                    // Page-protection violation
                    "present"
                } else {
                    "not present"
                },
                if info.error_code & 0b1000 != 0 {
                    "invalid"
                } else {
                    "valid"
                }
            );
        }
        _ => {
            error!("Not handled");
        }
    };
}

#[no_mangle]
extern "sysv64" fn int_handler_unimplemented() {
    panic!("unexpected interrupt!");
}

pub const BIT_FLAGS_INTGATE: u8 = 0b0000_1110u8;
pub const BIT_FLAGS_PRESENT: u8 = 0b1000_0000u8;
pub const BIT_FLAGS_DPL0: u8 = 0 << 5;
pub const BIT_FLAGS_DPL3: u8 = 3 << 5;

// IdtAttr = DPL
// 特権モードとかを指定するっぽい
// https://wiki.osdev.org/Security#Rings
#[repr(u8)]
#[derive(Copy, Clone)]
enum IdtAttr {
    _NotPresent = 0,
    IntGateDPL0 = BIT_FLAGS_INTGATE | BIT_FLAGS_PRESENT | BIT_FLAGS_DPL0,
    IntGateDPL3 = BIT_FLAGS_INTGATE | BIT_FLAGS_PRESENT | BIT_FLAGS_DPL3,
}

// 割り込み番号ごとのデータ
// https://wiki.osdev.org/Interrupt_Descriptor_Table#Structure_on_x86-64
/**
* IDT（テーブル）
 ├─ Gate Descriptor（例: 0番: Divide Error用）
 ├─ Gate Descriptor（例: 1番: Debug用）
 ├─ Gate Descriptor（例: 2番: NMI用）
 └─ ...（最大256個）
*/
#[repr(C, packed)]
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub struct IdtDescriptor {
    offset_low: u16,
    segment_selector: u16,
    ist_index: u8,
    // ハンドラの種類など
    attr: IdtAttr,
    offset_mid: u16,
    offset_high: u32,
    _reserved: u32,
}
const _: () = assert!(size_of::<IdtDescriptor>() == 16);

impl IdtDescriptor {
    fn new(
        segment_selector: u16,
        ist_index: u8,
        attr: IdtAttr,
        f: unsafe extern "sysv64" fn(),
    ) -> Self {
        let handler_addr = f as *const unsafe extern "sysv64" fn() as usize;
        Self {
            offset_low: handler_addr as u16,
            segment_selector,
            ist_index,
            attr,
            offset_mid: (handler_addr >> 16) as u16,
            offset_high: (handler_addr >> 32) as u32,
            _reserved: 0,
        }
    }
}

#[allow(dead_code)]
#[repr(C, packed)]
#[derive(Debug)]
struct IdtrParameters {
    limit: u16,
    base: *const IdtDescriptor,
}
const _: () = assert!(size_of::<IdtrParameters>() == 10);
const _: () = assert!(offset_of!(IdtrParameters, base) == 2);

pub struct Idt {
    #[allow(dead_code)]
    entries: Pin<Box<[IdtDescriptor; 0x100]>>,
}
impl Idt {
    pub fn new(segment_selector: u16) -> Self {
        // 各割り込み用のIdtDescriptorを作成する
        let mut entries = [IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            int_handler_unimplemented,
        ); 0x100];
        // Breakpoint Exception
        entries[3] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL3,
            interrupt_entrypoint3,
        );
        // Invalid Opcode Exception
        entries[6] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint6,
        );
        // Double Fault Exception
        entries[8] = IdtDescriptor::new(
            segment_selector,
            2,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint8,
        );
        // General Protection Fault
        entries[13] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint13,
        );
        // Page Fault
        entries[14] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint14,
        );
        entries[32] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint32,
        );
        let limit = size_of_val(&entries) as u16;
        // アドレスを固定
        let entries = Box::pin(entries);
        let params = IdtrParameters {
            limit,
            base: entries.as_ptr(),
        };
        info!("Loading IDT: {params:?}");
        unsafe {
            // Load IDT
            asm!("lidt [rcx]", in("rcx") &params);
        }
        Self { entries }
    }
}

// TSS（Task State Segment）の中にIST（Interrupt Stack Table）を定義する
#[repr(C, packed)]
struct TaskStateSegment64Inner {
    _reserved0: u32,
    _rsp: [u64; 3],
    _ist: [u64; 8], // IST1〜IST7にIST用スタックのアドレスを設定する
    _reserved1: [u16; 5],
    _io_map_base: u16,
}
const _: () = assert!(size_of::<TaskStateSegment64Inner>() == 104);

pub struct TaskStateSegment64 {
    inner: Pin<Box<TaskStateSegment64Inner>>,
}

impl TaskStateSegment64 {
    pub fn phys_addr(&self) -> u64 {
        self.inner.as_ref().get_ref() as *const TaskStateSegment64Inner as u64
    }
    // IST（割り込み時用の）スタック分のメモリを確保して、そのスタックの先頭アドレスを返す
    unsafe fn alloc_interrupt_stack() -> u64 {
        const HANDLER_STACK_SIZE: usize = 64 * 1024;
        let stack = Box::new([0u8; HANDLER_STACK_SIZE]);
        let rsp = unsafe { stack.as_ptr().add(HANDLER_STACK_SIZE) as u64 };
        core::mem::forget(stack); // 所有権を放棄
        rsp
    }
    pub fn new() -> Self {
        let rsp0 = unsafe { Self::alloc_interrupt_stack() };
        let mut ist = [0u64; 8];
        // 1~7までのISTにスタックを割り当てる
        for ist in ist[1..=7].iter_mut() {
            *ist = unsafe { Self::alloc_interrupt_stack() };
        }
        let tss64 = TaskStateSegment64Inner {
            _reserved0: 0,
            _rsp: [rsp0, 0, 0],
            _ist: ist,
            _reserved1: [0; 5],
            _io_map_base: 0,
        };
        let this = Self {
            inner: Box::pin(tss64),
        };
        info!("TSS64 created @ {:#X}", this.phys_addr());
        this
    }
}
impl Drop for TaskStateSegment64 {
    fn drop(&mut self) {
        panic!("TSS64 being dropped!");
    }
}

pub const BIT_TYPE_CODE: u64 = 0b10u64 << 43; // コード領域
pub const BIT_TYPE_DATA: u64 = 0b11u64 << 43; // データ領域

pub const BIT_PRESENT: u64 = 1u64 << 47;
pub const BIT_CS_LONG_MODE: u64 = 1u64 << 53;
pub const BIT_CS_READABLE: u64 = 1u64 << 53;
pub const BIT_DS_WRITABLE: u64 = 1u64 << 41;

pub const KERNEL_CS: u16 = 1 << 3;
pub const KERNEL_DS: u16 = 2 << 3;
pub const TSS64_SEL: u16 = 3 << 3;

#[repr(u64)]
enum GdtAttr {
    KernelCode = BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE,
    KernelData = BIT_TYPE_DATA | BIT_PRESENT | BIT_DS_WRITABLE,
}

pub struct GdtSegmentDescriptor {
    value: u64,
}

impl GdtSegmentDescriptor {
    const fn null() -> Self {
        Self { value: 0 }
    }
    const fn new(attr: GdtAttr) -> Self {
        Self { value: attr as u64 }
    }
}

impl fmt::Display for GdtSegmentDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:#018X}", self.value)
    }
}

#[repr(C, packed)]
#[allow(dead_code)]
struct TaskStateSegment64Descriptor {
    limit_low: u16,
    base_low: u16,
    base_mid_low: u8,
    attr1: u16,
    base_mid_high: u8,
    base_high: u32,
    reserved: u32,
}

impl TaskStateSegment64Descriptor {
    const fn new(base_addr: u64) -> Self {
        Self {
            limit_low: size_of::<TaskStateSegment64Inner>() as u16,
            base_low: (base_addr & 0xFFFF) as u16,
            base_mid_low: ((base_addr >> 16) & 0xFF) as u8,
            attr1: 0b1000_0000_1000_1001,
            base_mid_high: ((base_addr >> 24) & 0xFF) as u8,
            base_high: ((base_addr >> 32) & 0xFFFFFFFF) as u32,
            reserved: 0,
        }
    }
}
const _: () = assert!(size_of::<TaskStateSegment64Descriptor>() == 16);

/*
 * GDT（テーブル）
 * ├─ Segment Descriptor（例: 0番: NULLセグメント）
 * ├─ Segment Descriptor（例: 1番: カーネルコードセグメント）
 * ├─ Segment Descriptor（例: 2番: カーネルデータ
 * └─ TSS Descriptor（例: 3番: TSSセグメント） →割り込み時のスタック切り替え制御
 */

// https://wiki.osdev.org/GDT_Tutorial#Small_Kernel_Setup
#[allow(dead_code)]
#[repr(C, packed)]
pub struct Gdt {
    null_segment: GdtSegmentDescriptor,
    kernel_code_segment: GdtSegmentDescriptor,
    kernel_data_segment: GdtSegmentDescriptor,
    task_state_segment: TaskStateSegment64Descriptor,
}
const _: () = assert!(size_of::<Gdt>() == 40);

#[allow(dead_code)]
#[repr(C, packed)]
struct GdtParameteres {
    limit: u16,
    base: *const Gdt,
}

#[allow(dead_code)]
pub struct GdtWrapper {
    inner: Pin<Box<Gdt>>,
    tss64: TaskStateSegment64,
}

impl GdtWrapper {
    pub fn load(&self) {
        let params = GdtParameteres {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: self.inner.as_ref().get_ref() as *const Gdt,
        };
        info!("Loading GDT @ {:#018X}", params.base as u64);

        unsafe {
            asm!("lgdt [rcx]", in("rcx") &params);
        }
        // TSSがGDTの3番目にあるので、3*8=0x18を指定する
        info!("Loading TSS (selector = {:#X} )", TSS64_SEL);
        unsafe {
            asm!("ltr cx", in("cx") TSS64_SEL);
        }
    }
}
impl Default for GdtWrapper {
    fn default() -> Self {
        let tss64 = TaskStateSegment64::new();
        let gdt = Gdt {
            null_segment: GdtSegmentDescriptor::null(),
            kernel_code_segment: GdtSegmentDescriptor::new(GdtAttr::KernelCode),
            kernel_data_segment: GdtSegmentDescriptor::new(GdtAttr::KernelData),
            task_state_segment: TaskStateSegment64Descriptor::new(tss64.phys_addr()),
        };
        let gdt = Box::pin(gdt);
        Self { inner: gdt, tss64 }
    }
}

pub fn init_exceptions() -> (GdtWrapper, Idt) {
    unsafe {
        asm!("cli");
    }
    let gdt = GdtWrapper::default();
    gdt.load();
    info!("GDT initilized");
    unsafe {
        write_cs(KERNEL_CS);
        write_ss(KERNEL_DS);
        write_es(KERNEL_DS); // ok
        write_ds(KERNEL_DS);
        write_fs(KERNEL_DS);
        write_gs(KERNEL_DS);
    }
    info!("Segment initilized");
    let idt = Idt::new(KERNEL_CS);
    unsafe {
        asm!("sti");
    }
    (gdt, idt)
}

pub fn trigger_debug_interrupt() {
    unsafe { asm!("int3") }
}
