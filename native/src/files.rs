//! Files the builders read on the main thread: textures (imported `res://` textures through
//! `ResourceLoader`, other images through `Image`, members of a zip archive) and resources
//! (materials, scenes, scripts), resolved as the GDScript runtime's `DataLoader` resolves them.

use crate::diag::Diag;
use crate::gd::{cleanup_path, get_extension, path_join};
use godot::classes::{FileAccess, Image, ImageTexture, ResourceLoader, Texture2D, ZipReader};
use godot::global::Error as GdError;
use godot::prelude::*;
use std::collections::HashMap;

/// `DataLoader._resolve_path_in_file_system`: the name as given, else relative to `base`.
pub fn resolve_in_fs(file: &str, base: &str) -> Option<String> {
    if FileAccess::file_exists(file) {
        return Some(file.to_string());
    }
    let joined = path_join(base, file);
    FileAccess::file_exists(joined.as_str()).then_some(joined)
}

/// `DataLoader.load_resource_from_file`.
pub fn load_resource(file: &str, base: &str, diag: &mut Diag) -> Option<Gd<Resource>> {
    match resolve_in_fs(file, base) {
        Some(p) => ResourceLoader::singleton().load(p.as_str()),
        None => {
            diag.error(format!("ERROR: Resource file '{file}' not found."));
            None
        }
    }
}

/// Textures by resolved path, for the imports one `TiledTileSets` serves.
#[derive(Default)]
pub struct Textures {
    cache: HashMap<String, Gd<Texture2D>>,
    zip: Option<(String, Gd<ZipReader>)>,
}

impl Textures {
    /// Forgets every texture (and closes a zip archive).
    pub fn clear(&mut self) {
        self.cache.clear();
        self.close_zip();
    }

    /// Closes the zip archive opened for an import.
    pub fn close_zip(&mut self) {
        if let Some((_, mut r)) = self.zip.take() {
            r.close();
        }
    }

    /// `DataLoader.load_image`: the image `file` referenced from a file in `base`, from the file
    /// system or (when `zip` is not empty) from that archive.
    pub fn load(
        &mut self,
        file: &str,
        base: &str,
        zip: &str,
        diag: &mut Diag,
    ) -> Option<Gd<Texture2D>> {
        if zip.is_empty() {
            self.load_from_fs(file, base, diag)
        } else {
            self.load_from_zip(file, base, zip, diag)
        }
    }

    fn load_from_fs(&mut self, file: &str, base: &str, diag: &mut Diag) -> Option<Gd<Texture2D>> {
        let Some(checked) = resolve_in_fs(file, base) else {
            diag.error(format!("ERROR: Image file '{file}' not found."));
            return None;
        };
        if let Some(t) = self.cache.get(&checked) {
            return Some(t.clone());
        }
        let loader = ResourceLoader::singleton();
        let texture: Gd<Texture2D> = if loader
            .clone()
            .exists_ex(checked.as_str())
            .type_hint("Image")
            .done()
        {
            let res = ResourceLoader::singleton().load(checked.as_str())?;
            match res.try_cast::<Texture2D>() {
                Ok(t) => t,
                Err(_) => {
                    diag.error(format!(
                        "ERROR: Image file '{checked}' could not be loaded."
                    ));
                    return None;
                }
            }
        } else {
            let Some(image) = Image::load_from_file(checked.as_str()) else {
                diag.error(format!(
                    "ERROR: Image file '{checked}' could not be loaded."
                ));
                return None;
            };
            ImageTexture::create_from_image(&image)?.upcast()
        };
        self.cache.insert(checked, texture.clone());
        Some(texture)
    }

    fn reader(&mut self, zip: &str, diag: &mut Diag) -> Option<Gd<ZipReader>> {
        if let Some((p, r)) = &self.zip {
            if p == zip {
                return Some(r.clone());
            }
        }
        self.close_zip();
        let mut r = ZipReader::new_gd();
        if r.open(zip) != GdError::OK {
            diag.error(format!("ERROR: Unable to open zip file '{zip}'."));
            return None;
        }
        self.zip = Some((zip.to_string(), r.clone()));
        Some(r)
    }

    fn load_from_zip(
        &mut self,
        file: &str,
        base: &str,
        zip: &str,
        diag: &mut Diag,
    ) -> Option<Gd<Texture2D>> {
        let mut reader = self.reader(zip, diag)?;
        let member = if reader.file_exists(file) {
            file.to_string()
        } else {
            let p = cleanup_path(&path_join(base, file));
            if reader.file_exists(p.as_str()) {
                p
            } else {
                diag.error(format!(
                    "ERROR: Image file '{file}' not found in zip file '{zip}'."
                ));
                return None;
            }
        };
        let key = format!("{zip}::{member}");
        if let Some(t) = self.cache.get(&key) {
            return Some(t.clone());
        }
        let bytes = reader.read_file(member.as_str());
        let mut image = Image::new_gd();
        let err = match get_extension(file).to_lowercase().as_str() {
            "png" => image.load_png_from_buffer(&bytes),
            "jpg" | "jpeg" => image.load_jpg_from_buffer(&bytes),
            "bmp" => image.load_bmp_from_buffer(&bytes),
            "webp" => image.load_webp_from_buffer(&bytes),
            "tga" => image.load_tga_from_buffer(&bytes),
            _ => GdError::ERR_FILE_UNRECOGNIZED,
        };
        if err != GdError::OK {
            diag.error(format!(
                "ERROR: Image file '{member}' in zip file '{zip}' could not be loaded."
            ));
            return None;
        }
        let texture: Gd<Texture2D> = ImageTexture::create_from_image(&image)?.upcast();
        self.cache.insert(key, texture.clone());
        Some(texture)
    }
}
