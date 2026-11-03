//! EGL display + GLES context for the translucent UI overlay.
//! RGBA8888 with alpha so the overlay is transparent over the hardware video plane.

use egui_glow::glow;
use khronos_egl as egl;
use ndk::native_window::NativeWindow;
use std::sync::Arc;

type Egl = egl::DynamicInstance<egl::EGL1_4>;

pub struct Window {
    _win: NativeWindow,
    surface: egl::Surface,
}

pub struct Core {
    egl: Egl,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
    pbuffer: Option<egl::Surface>,
    pub gl: Arc<glow::Context>,
}

impl Core {
    pub fn new() -> Option<Core> {
        unsafe {
            let egl = Egl::load_required()
                .map_err(|e| log::error!("libEGL: {e}"))
                .ok()?;
            let display = egl.get_display(egl::DEFAULT_DISPLAY)?;
            egl.initialize(display).ok()?;
            egl.bind_api(egl::OPENGL_ES_API).ok()?;
            let attribs = |renderable: egl::Int, st: egl::Int| {
                [
                    egl::RENDERABLE_TYPE,
                    renderable,
                    egl::SURFACE_TYPE,
                    st,
                    egl::RED_SIZE,
                    8,
                    egl::GREEN_SIZE,
                    8,
                    egl::BLUE_SIZE,
                    8,
                    egl::ALPHA_SIZE,
                    8,
                    egl::NONE,
                ]
            };
            let mut chosen = None;
            'outer: for (bit, version) in [(egl::OPENGL_ES3_BIT, 3), (egl::OPENGL_ES2_BIT, 2)] {
                for st in [egl::WINDOW_BIT | egl::PBUFFER_BIT, egl::WINDOW_BIT] {
                    if let Ok(Some(c)) = egl.choose_first_config(display, &attribs(bit, st)) {
                        chosen = Some((c, version, st));
                        break 'outer;
                    }
                }
            }
            let (config, version, st) = chosen?;
            let context = egl
                .create_context(
                    display,
                    config,
                    None,
                    &[egl::CONTEXT_CLIENT_VERSION, version, egl::NONE],
                )
                .ok()?;
            let pbuffer = (st & egl::PBUFFER_BIT != 0)
                .then(|| {
                    egl.create_pbuffer_surface(
                        display,
                        config,
                        &[egl::WIDTH, 1, egl::HEIGHT, 1, egl::NONE],
                    )
                    .ok()
                })
                .flatten();
            if egl
                .make_current(display, pbuffer, pbuffer, Some(context))
                .is_err()
            {
                log::error!("eglMakeCurrent (pbuffer={}) failed", pbuffer.is_some());
                return None;
            }
            let gl = Arc::new(glow::Context::from_loader_function(|name| {
                egl.get_proc_address(name)
                    .map_or(std::ptr::null(), |p| p as *const _)
            }));
            Some(Core {
                egl,
                display,
                config,
                context,
                pbuffer,
                gl,
            })
        }
    }

    pub fn create(&self, win: NativeWindow) -> Option<Window> {
        unsafe {
            let visual = self
                .egl
                .get_config_attrib(self.display, self.config, egl::NATIVE_VISUAL_ID)
                .unwrap_or(0);
            if visual > 0 {
                ndk_sys::ANativeWindow_setBuffersGeometry(win.ptr().as_ptr(), 0, 0, visual);
            }
            let s = self.egl.create_window_surface(
                self.display,
                self.config,
                win.ptr().as_ptr() as egl::NativeWindowType,
                None,
            );
            match s {
                Ok(surface) => Some(Window { _win: win, surface }),
                Err(e) => {
                    log::error!("eglCreateWindowSurface: {e}");
                    None
                }
            }
        }
    }

    pub fn destroy(&self, w: Window) {
        self.make_current(None);
        if let Err(e) = self.egl.destroy_surface(self.display, w.surface) {
            log::warn!("eglDestroySurface: {e}");
        }
    }

    pub fn make_current(&self, w: Option<&Window>) -> bool {
        let s = w.map(|w| w.surface).or(self.pbuffer);
        self.egl
            .make_current(self.display, s, s, Some(self.context))
            .is_ok()
    }

    pub fn swap(&self, w: &Window) -> bool {
        self.egl.swap_buffers(self.display, w.surface).is_ok()
    }

    pub fn size(&self, w: &Window) -> Option<[u32; 2]> {
        let q = |a| self.egl.query_surface(self.display, w.surface, a).ok();
        let (x, y) = (q(egl::WIDTH)?, q(egl::HEIGHT)?);
        (x > 0 && y > 0).then_some([x as u32, y as u32])
    }
}
