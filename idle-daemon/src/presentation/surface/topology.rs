// SPDX-License-Identifier: MIT

use idle_api::OutputLayout;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MonitorTopology {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: i32,
    pub refresh_mhz: u32,
}

#[derive(Debug, Clone)]
pub struct DisplayTopologyMap {
    pub monitors: Vec<MonitorTopology>,
    pub independent_rendering: bool,
}

impl DisplayTopologyMap {
    pub fn build(layouts: &[OutputLayout]) -> Self {
        let independent_rendering = idle_api::env_var_first(&["IDLE_INDEPENDENT_RENDERING"])
            .map(|val| val == "1" || val.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let custom_layouts = idle_api::env_var_first(&["IDLE_CUSTOM_LAYOUTS"])
            .map(|s| parse_custom_layouts(&s))
            .unwrap_or_default();

        let mut monitors = Vec::new();
        for layout in layouts {
            let mut x = layout.x;
            let mut y = layout.y;
            let mut w = layout.width;
            let mut h = layout.height;
            let mut scale = layout.scale;

            if let Some(custom) = custom_layouts.get(&layout.id) {
                if let Some(cx) = custom.x {
                    x = cx;
                }
                if let Some(cy) = custom.y {
                    y = cy;
                }
                if let Some(cw) = custom.w {
                    w = cw;
                }
                if let Some(ch) = custom.h {
                    h = ch;
                }
                if let Some(cs) = custom.scale {
                    scale = cs;
                }
            }

            monitors.push(MonitorTopology {
                id: layout.id,
                x,
                y,
                width: w,
                height: h,
                scale,
                refresh_mhz: layout.refresh_mhz,
            });
        }

        Self {
            monitors,
            independent_rendering,
        }
    }
}

#[derive(Default)]
struct CustomOverride {
    x: Option<i32>,
    y: Option<i32>,
    w: Option<u32>,
    h: Option<u32>,
    scale: Option<i32>,
}

fn parse_custom_layouts(s: &str) -> HashMap<u32, CustomOverride> {
    let mut map = HashMap::new();
    // format: "id:x,y,w,h,scale;..."
    for entry in s.split(';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.split(':').collect();
        if parts.len() == 2
            && let Ok(id) = parts[0].parse::<u32>()
        {
            let coords: Vec<&str> = parts[1].split(',').collect();
            let mut ov = CustomOverride::default();
            if !coords.is_empty() {
                ov.x = coords[0].parse().ok();
            }
            if coords.len() >= 2 {
                ov.y = coords[1].parse().ok();
            }
            if coords.len() >= 3 {
                ov.w = coords[2].parse().ok();
            }
            if coords.len() >= 4 {
                ov.h = coords[3].parse().ok();
            }
            if coords.len() >= 5 {
                ov.scale = coords[4].parse().ok();
            }
            map.insert(id, ov);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_custom_layouts_empty() {
        let map = parse_custom_layouts("");
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_custom_layouts_valid() {
        let map = parse_custom_layouts("1:100,200,800,600,2;2:0,0,1920,1080,1");
        assert_eq!(map.len(), 2);
        let ov1 = map.get(&1).expect("id 1 present");
        assert_eq!(ov1.x, Some(100));
        assert_eq!(ov1.y, Some(200));
        assert_eq!(ov1.w, Some(800));
        assert_eq!(ov1.h, Some(600));
        assert_eq!(ov1.scale, Some(2));

        let ov2 = map.get(&2).expect("id 2 present");
        assert_eq!(ov2.x, Some(0));
        assert_eq!(ov2.y, Some(0));
        assert_eq!(ov2.w, Some(1920));
        assert_eq!(ov2.h, Some(1080));
        assert_eq!(ov2.scale, Some(1));
    }

    #[test]
    fn parse_skips_malformed_entries() {
        let map = parse_custom_layouts("nope;1:10,20;:bad;2;3:1,2,3,4,5,extra");
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&1).map(|o| (o.x, o.y)), Some((Some(10), Some(20))));
        assert_eq!(map.get(&3).and_then(|o| o.scale), Some(5));
    }

    #[test]
    fn parse_partial_coords_leaves_unset() {
        let map = parse_custom_layouts("7:42");
        let ov = map.get(&7).expect("id 7");
        assert_eq!(ov.x, Some(42));
        assert!(ov.y.is_none());
        assert!(ov.w.is_none());
        assert!(ov.h.is_none());
        assert!(ov.scale.is_none());
    }

    #[test]
    fn parse_is_idempotent_for_last_id_wins() {
        let map = parse_custom_layouts("1:1,2,3,4,1;1:9,8,7,6,5");
        let ov = map.get(&1).expect("id 1");
        assert_eq!(ov.x, Some(9));
        assert_eq!(ov.y, Some(8));
        assert_eq!(ov.w, Some(7));
        assert_eq!(ov.h, Some(6));
        assert_eq!(ov.scale, Some(5));
    }
}
