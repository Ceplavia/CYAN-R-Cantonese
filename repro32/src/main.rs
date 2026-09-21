use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Diagnostics::Debug::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_I386;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// R-Cantonese text service CLSID
const CLSID_RCANTONESE: GUID = GUID::from_values(0xd2291a80, 0x84d8, 0x4641, [0x9a, 0xb2, 0xbd, 0xd1, 0x47, 0x2c, 0x84, 0x6b]);

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, w, l)
}

// same FFI declarations as rcantonese/src/db.rs — isolate whether raw-dylib
// imports of winsqlite3 are the crash source on x86
#[repr(C)]
struct Sqlite3 {
    _p: [u8; 0],
}
#[repr(C)]
struct Sqlite3Stmt {
    _p: [u8; 0],
}
#[link(name = "winsqlite3", kind = "raw-dylib")]
unsafe extern "C" {
    fn sqlite3_open_v2(f: *const i8, db: *mut *mut Sqlite3, flags: i32, vfs: *const i8) -> i32;
    fn sqlite3_busy_timeout(db: *mut Sqlite3, ms: i32) -> i32;
    fn sqlite3_exec(
        db: *mut Sqlite3,
        sql: *const i8,
        cb: Option<unsafe extern "C" fn(*mut core::ffi::c_void, i32, *mut *mut i8, *mut *mut i8) -> i32>,
        ctx: *mut core::ffi::c_void,
        err: *mut *mut i8,
    ) -> i32;
    fn sqlite3_close_v2(db: *mut Sqlite3) -> i32;
    fn sqlite3_prepare16_v2(db: *mut Sqlite3, sql: *const u16, n: i32, st: *mut *mut Sqlite3Stmt, tail: *mut *const u16) -> i32;
    fn sqlite3_step(st: *mut Sqlite3Stmt) -> i32;
    fn sqlite3_finalize(st: *mut Sqlite3Stmt) -> i32;
}

unsafe fn sqlite_selftest() {
    eprintln!("sqlite selftest...");
    let mut db: *mut Sqlite3 = std::ptr::null_mut();
    let path = std::ffi::CString::new("C:\\Users\\shagg\\AppData\\Local\\Temp\\repro32-test.sqlite3").unwrap();
    let r = sqlite3_open_v2(path.as_ptr(), &mut db, 0x2 | 0x4 | 0x10000, std::ptr::null());
    eprintln!("open_v2 -> {} db={:?}", r, db);
    if r != 0 {
        return;
    }
    eprintln!("busy_timeout -> {}", sqlite3_busy_timeout(db, 250));
    let sql = std::ffi::CString::new("CREATE TABLE IF NOT EXISTS t(a);").unwrap();
    eprintln!("exec -> {}", sqlite3_exec(db, sql.as_ptr(), None, std::ptr::null_mut(), std::ptr::null_mut()));
    let w: Vec<u16> = "SELECT 1;".encode_utf16().chain(Some(0)).collect();
    let mut st: *mut Sqlite3Stmt = std::ptr::null_mut();
    eprintln!("prepare16 -> {}", sqlite3_prepare16_v2(db, w.as_ptr(), -1, &mut st, std::ptr::null_mut()));
    eprintln!("step -> {}", sqlite3_step(st));
    eprintln!("finalize -> {}", sqlite3_finalize(st));
    eprintln!("close -> {}", sqlite3_close_v2(db));
    eprintln!("sqlite selftest survived");
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

unsafe fn dump_path() -> String {
    let mut buf = [0u16; 260];
    let n = GetTempPathW(Some(&mut buf));
    let dir = String::from_utf16_lossy(&buf[..n as usize]);
    format!("{}repro32-crash.dmp", dir.trim_end_matches('\\'))
}

unsafe fn our_dll_range() -> Option<(usize, usize)> {
    let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, GetCurrentProcessId()).ok()?;
    let mut me = MODULEENTRY32W::default();
    me.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
    let mut range = None;
    if Module32FirstW(snap, &mut me).is_ok() {
        loop {
            let name = String::from_utf16_lossy(&me.szModule[..me
                .szModule
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(0)])
                .to_lowercase();
            if name.contains("r_cantonese") {
                let base = me.modBaseAddr as usize;
                range = Some((base, base + me.modBaseSize as usize));
                eprintln!("our dll: base={:#x} size={:#x}", base, me.modBaseSize);
            }
            if Module32NextW(snap, &mut me).is_err() {
                break;
            }
        }
    }
    let _ = CloseHandle(snap);
    range
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    let code = (*(*info).ExceptionRecord).ExceptionCode.0;
    if code != 0xC0000005u32 as i32 && code != 0xC000001Du32 as i32 && code != 0xC00000FDu32 as i32
    {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let ctx = (*info).ContextRecord;
    let eip = (*ctx).Eip;
    let esp = (*ctx).Esp;
    let ebp = (*ctx).Ebp;
    eprintln!("\n=== CRASH: code={:#010x} EIP={:#010x} ESP={:#010x} EBP={:#010x} ===", code as u32, eip, esp, ebp);
    if let Some((base, end)) = our_dll_range() {
        eprintln!("EIP in dll? {}", eip as usize >= base && (eip as usize) < end);
    }
    // proper unwind via dbghelp + PDB symbols
    let proc = GetCurrentProcess();
    let _ = SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES | SYMOPT_UNDNAME);
    unsafe extern "system" fn ft_access(h: HANDLE, a: u64) -> *mut core::ffi::c_void {
        SymFunctionTableAccess64(h, a)
    }
    unsafe extern "system" fn mod_base(h: HANDLE, a: u64) -> u64 {
        SymGetModuleBase64(h, a)
    }
    if SymInitialize(proc, None, true).is_ok() {
        // force-load our dll's PDB
        if let Some((base, size)) = our_dll_range() {
            let dll_path = wide("D:\\rust_proj\\r-cantonese\\target\\i686-pc-windows-msvc\\debug\\r_cantonese.dll");
            let loaded = SymLoadModuleExW(
                proc,
                None,
                PCWSTR(dll_path.as_ptr()),
                None,
                base as u64,
                size as u32,
                None,
                None,
            );
            eprintln!("SymLoadModuleEx -> {:#x} (err {:?})", loaded, GetLastError());
        }
        let mut frame = STACKFRAME64::default();
        frame.AddrPC.Mode = AddrModeFlat;
        frame.AddrPC.Offset = eip as u64;
        frame.AddrFrame.Mode = AddrModeFlat;
        frame.AddrFrame.Offset = ebp as u64;
        frame.AddrStack.Mode = AddrModeFlat;
        frame.AddrStack.Offset = esp as u64;
        let mut ctx_copy = *ctx;
        for i in 0..40 {
            let ok = StackWalk64(
                IMAGE_FILE_MACHINE_I386.0 as _,
                proc,
                GetCurrentThread(),
                &mut frame,
                &mut ctx_copy as *mut _ as *mut _,
                None,
                Some(ft_access),
                Some(mod_base),
                None,
            );
            if !ok.as_bool() || frame.AddrPC.Offset == 0 {
                break;
            }
            let pc = frame.AddrPC.Offset;
            let mut sym_buf = [0u8; 512];
            let sym = sym_buf.as_mut_ptr() as *mut SYMBOL_INFO;
            (*sym).SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
            (*sym).MaxNameLen = 255;
            let mut disp = 0u64;
            if SymFromAddr(proc, pc, Some(&mut disp), sym).is_ok() {
                let name_len = (*sym).NameLen as usize;
                let name = String::from_utf8_lossy(std::slice::from_raw_parts(
                    (*sym).Name.as_ptr() as *const u8,
                    name_len.min(255),
                ));
                eprintln!("  #{:02} {:#010x} {}+{:#x}", i, pc, name, disp);
            } else {
                eprintln!("  #{:02} {:#010x} <no sym>", i, pc);
            }
        }
    } else {
        eprintln!("SymInitialize failed");
    }
    let path = dump_path();
    let name = wide(&path);
    let file = CreateFileW(
        PCWSTR(name.as_ptr()),
        FILE_GENERIC_WRITE.0,
        FILE_SHARE_WRITE,
        None,
        CREATE_ALWAYS,
        FILE_ATTRIBUTE_NORMAL,
        None,
    );
    if let Ok(file) = file {
        let mut ex = MINIDUMP_EXCEPTION_INFORMATION {
            ThreadId: GetCurrentThreadId(),
            ExceptionPointers: info,
            ClientPointers: FALSE,
        };
        let _ = MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file,
            MiniDumpWithFullMemory,
            Some(&mut ex),
            None,
            None,
        );
        let _ = CloseHandle(file);
        eprintln!("dump written: {}", path);
    }
    EXCEPTION_EXECUTE_HANDLER
}

unsafe fn run() -> windows::core::Result<()> {
    CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    eprintln!("coinit ok");

    // a GUI thread (window + queue) before activating TSF — console-only
    // threads get E_INVALIDARG from AdviseKeyEventSink
    let cls = wide("Repro32Wnd");
    let wc = WNDCLASSW {
        lpszClassName: PCWSTR(cls.as_ptr()),
        lpfnWndProc: Some(wndproc),
        ..Default::default()
    };
    RegisterClassW(&wc);
    let _hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        PCWSTR(cls.as_ptr()),
        PCWSTR(wide("repro32").as_ptr()),
        WINDOW_STYLE::default(),
        0, 0, 0, 0,
        Some(HWND_MESSAGE),
        None,
        None,
        None,
    )?;
    let mut msg = MSG::default();
    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE);
    eprintln!("window ok");

    let tm: ITfThreadMgr = CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_ALL)?;
    eprintln!("threadmgr ok");
    let tid = tm.Activate()?;
    eprintln!("tm activate ok tid={}", tid);

    // what does a 32-bit process actually see under HKCR\CLSID\{x}?
    for path in [
        "CLSID\\{D2291A80-84D8-4641-9AB2-BDD1472C846B}\\InProcServer32",
        "CLSID\\WOW6432Node\\CLSID\\{D2291A80-84D8-4641-9AB2-BDD1472C846B}\\InProcServer32",
        "SOFTWARE\\Classes\\WOW6432Node\\CLSID\\{D2291A80-84D8-4641-9AB2-BDD1472C846B}\\InProcServer32",
    ] {
        let p = wide(path);
        let mut k = HKEY::default();
        let r = RegOpenKeyExW(HKEY_CLASSES_ROOT, PCWSTR(p.as_ptr()), Some(0), KEY_READ, &mut k);
        eprintln!("reg HKCR\\{} -> {:?}", path, r);
        if r.is_ok() {
            let mut buf = [0u16; 512];
            let mut n = (buf.len() * 2) as u32;
            if RegQueryValueExW(k, None, None, None, Some(buf.as_mut_ptr() as *mut u8), Some(&mut n)).is_ok() {
                eprintln!("   = {}", String::from_utf16_lossy(&buf[..(n as usize / 2).saturating_sub(1)]));
            }
            let _ = RegCloseKey(k);
        }
    }

    let tip: ITfTextInputProcessorEx =
        CoCreateInstance(&CLSID_RCANTONESE, None, CLSCTX_INPROC_SERVER)?;
    eprintln!("tip cocreate ok");

    tip.ActivateEx(&tm, tid, 0)?;
    eprintln!("ActivateEx ok");

    let doc = tm.CreateDocumentMgr()?;
    eprintln!("docmgr ok");

    let mut ctx: Option<ITfContext> = None;
    let mut edit = 0u32;
    doc.CreateContext(tid, 0, None, &mut ctx, &mut edit)?;
    eprintln!("ctx ok edit={}", edit);
    let ctx = ctx.unwrap();

    doc.Push(&ctx)?;
    eprintln!("push ok");

    // pump messages so PostMessage-based deferred work runs
    let mut msg = MSG::default();
    for _ in 0..20 {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Sleep(50);
    }
    eprintln!("pump done — survived");

    doc.Pop(TF_POPF_ALL)?;
    let _ = tip.Deactivate();
    tm.Deactivate()?;
    eprintln!("clean shutdown ok");
    Ok(())
}

fn main() {
    unsafe {
        AddVectoredExceptionHandler(1, Some(veh));
        if std::env::var("REPRO32_NOTHING").is_ok() {
            eprintln!("nothing — returning");
            return;
        }
        if std::env::var("REPRO32_SQLITE_ONLY").is_ok() {
            sqlite_selftest();
            return;
        }
        match run() {
            Ok(()) => eprintln!("=== OK ==="),
            Err(e) => eprintln!("=== HRESULT error: {:?} ===", e),
        }
    }
}
