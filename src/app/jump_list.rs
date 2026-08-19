//! Windows taskbar jump list: right-click the taskbar icon → "新建窗口",
//! launching `meatshell --new-window`. The launch is forwarded to the
//! running primary instance over the single-instance IPC socket (see
//! `single_instance.rs`), so the entry behaves like Chrome's "new window"
//! task instead of spawning a second process.
//!
//! Registration runs at startup, before the first window is shown, and must
//! never block or fail startup: every error path degrades to a tracing warn.

use windows::core::{w, HRESULT, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
};

/// AppUserModelID used to attach the jump list to this application.
/// Must stay in sync with the identity Explorer uses for meatshell.
const APP_ID: &str = "meatshell";

/// Register the "新建窗口" user task on the taskbar jump list.
///
/// Failures are logged and swallowed — a missing jump list entry must never
/// keep the app from starting.
pub fn register_new_window_task() {
    if let Err(e) = register_inner() {
        tracing::warn!("jump list registration failed: {e}");
    }
}

fn register_inner() -> windows::core::Result<()> {
    unsafe {
        // The main thread has no COM apartment yet at this point in startup;
        // ignore S_FALSE (already initialized) and RPC_E_CHANGED_MODE (some
        // shell component initialized it differently) — either way COM is
        // usable from here.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // current_exe() also works when launched as a portable/AppImage-style
        // bare binary, so the jump list keeps pointing at the running image.
        let exe = std::env::current_exe().map_err(|e| {
            windows::core::Error::new(
                HRESULT(e.raw_os_error().unwrap_or(0)),
                format!("current_exe: {e}"),
            )
        })?;

        // Shell link: target = this exe, args = --new-window.
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL)?;
        link.SetPath(&HSTRING::from(exe.as_path()))?;
        link.SetArguments(w!("--new-window"))?;
        link.SetDescription(w!("新建窗口"))?;

        // User tasks collection holding the single link.
        let collection: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_ALL)?;
        collection.AddObject(&link)?;

        // Custom destination list: publish the task under our AppUserModelID.
        let list: ICustomDestinationList = CoCreateInstance(&DestinationList, None, CLSCTX_ALL)?;
        let _ = list.SetAppID(&HSTRING::from(APP_ID));
        let mut slots = 0u32;
        let _removed: IObjectArray = list.BeginList(&mut slots)?;
        list.AddUserTasks(&collection)?;
        list.CommitList()?;
        Ok(())
    }
}
