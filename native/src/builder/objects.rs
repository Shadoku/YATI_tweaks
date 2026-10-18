//! Objects: not built yet (the next step of stage 2); object layers come out empty.

use super::{Build, Obj};
use crate::tileset::Built;
use godot::prelude::*;
use std::rc::Rc;

impl Build<'_> {
    /// `handle_object`: nothing yet.
    pub(super) fn handle_object(
        &mut self,
        _obj: Obj,
        _layer_node: &Gd<Node>,
        _ts: &Rc<Built>,
        _offset: Vector2,
    ) {
    }

    /// `get_object_polygon`: nothing yet.
    pub(super) fn object_polygon(&mut self, _id: i64) -> Option<Variant> {
        None
    }
}
