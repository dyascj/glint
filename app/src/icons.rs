//! Icon textures. A background thread renders app/file icons, SF Symbols and
//! emoji through AppKit; the UI asks every frame and gets None until ready.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

use crate::macos::{self, Img};

/// Pixels per side: 32 pt icons at 2x look sharp at every size we draw.
const PX: usize = 64;

pub struct Icons {
    textures: HashMap<Img, TextureHandle>,
    requested: HashSet<Img>,
    tx: Sender<Img>,
    rx: Receiver<(Img, Option<Vec<u8>>)>,
}

impl Icons {
    pub fn new(ctx: &egui::Context) -> Icons {
        let (tx, jobs) = channel::<Img>();
        let (done, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for img in jobs {
                let rgba = macos::render(&img, PX);
                if done.send((img, rgba)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Icons { textures: HashMap::new(), requested: HashSet::new(), tx, rx }
    }

    /// Moves finished renders into textures; call once per frame.
    pub fn pump(&mut self, ctx: &egui::Context) {
        while let Ok((img, rgba)) = self.rx.try_recv() {
            if let Some(rgba) = rgba {
                let image = ColorImage::from_rgba_premultiplied([PX, PX], &rgba);
                let name = format!("{img:?}");
                self.textures.insert(img, ctx.load_texture(name, image, TextureOptions::LINEAR));
            }
        }
    }

    pub fn get(&mut self, img: &Img) -> Option<&TextureHandle> {
        if !self.textures.contains_key(img) && self.requested.insert(img.clone()) {
            let _ = self.tx.send(img.clone());
        }
        self.textures.get(img)
    }

    /// Queues renders ahead of time (all app icons at launch).
    pub fn warm(&mut self, imgs: impl IntoIterator<Item = Img>) {
        for img in imgs {
            let _ = self.get(&img);
        }
    }

    /// Paints `img` into `rect`; symbols are tinted with `tint`.
    pub fn paint(&mut self, painter: &egui::Painter, img: &Img, rect: egui::Rect, tint: egui::Color32) -> bool {
        let color = if matches!(img, Img::Symbol(_)) { tint } else { egui::Color32::WHITE };
        if let Some(t) = self.get(img) {
            painter.image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), color);
            true
        } else {
            false
        }
    }
}
