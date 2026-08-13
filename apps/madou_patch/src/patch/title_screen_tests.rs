use super::*;

fn decode_2bpp(tile: &[u8]) -> [u8; 64] {
    let mut result = [0u8; 64];
    for y in 0..8 {
        for x in 0..8 {
            let bit = 7 - x;
            result[y * 8 + x] = ((tile[y * 2] >> bit) & 1) | (((tile[y * 2 + 1] >> bit) & 1) << 1);
        }
    }
    result
}

fn decode_4bpp(tile: &[u8]) -> [u8; 64] {
    let mut result = decode_2bpp(tile);
    for y in 0..8 {
        for x in 0..8 {
            let bit = 7 - x;
            result[y * 8 + x] |= ((tile[16 + y * 2] >> bit) & 1) << 2;
            result[y * 8 + x] |= ((tile[16 + y * 2 + 1] >> bit) & 1) << 3;
        }
    }
    result
}

#[test]
fn tile_encoding_round_trips_all_indices() {
    let mut tile_2bpp = [0u8; 64];
    let mut tile_4bpp = [0u8; 64];
    for (index, pixel) in tile_2bpp.iter_mut().enumerate() {
        *pixel = (index % 4) as u8;
    }
    for (index, pixel) in tile_4bpp.iter_mut().enumerate() {
        *pixel = (index % 16) as u8;
    }
    assert_eq!(decode_2bpp(&encode_tile_2bpp(&tile_2bpp)), tile_2bpp);
    assert_eq!(decode_4bpp(&encode_tile_4bpp(&tile_4bpp)), tile_4bpp);
}

#[test]
fn subtitle_tiles_do_not_overlap_outline_budget() {
    let reserved = subtitle_reserved_tiles();
    let outline = bg3_outline_tiles(3312);
    assert_eq!(reserved.len(), 50);
    assert_eq!(outline.len(), 156);
    assert!(outline.iter().all(|tile| !reserved.contains(tile)));
}

#[test]
fn projection_valleys_split_five_subtitle_glyphs() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/title_concepts/components/daeyuchiwona_v2.png");
    let image = load_png(&path).expect("load subtitle master");
    let cropped = crop_alpha(&image).expect("crop subtitle master");
    let ranges = five_glyph_ranges(&cropped);
    assert_eq!(ranges.len(), 5);
    assert_eq!(ranges.first().unwrap().0, 0);
    assert_eq!(ranges.last().unwrap().1, cropped.width);
    assert!(ranges.windows(2).all(|pair| pair[0].1 == pair[1].0));
}

#[test]
fn fit_preserves_aspect_and_bounds() {
    let mut image = Image::transparent(40, 20);
    for y in 5..15 {
        for x in 10..30 {
            image.set(
                x,
                y,
                Pixel {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
            );
        }
    }
    let fitted = fit(&image, 100, 30).unwrap();
    assert_eq!((fitted.width, fitted.height), (60, 30));
    assert!(fitted.pixels.iter().all(|pixel| pixel.a == 255));
}

#[test]
fn rotate_nearest_turns_a_horizontal_row_counterclockwise() {
    let mut image = Image::transparent(3, 3);
    let left = Pixel {
        r: 32,
        g: 0,
        b: 0,
        a: 255,
    };
    let middle = Pixel {
        r: 64,
        g: 0,
        b: 0,
        a: 255,
    };
    let right = Pixel {
        r: 96,
        g: 0,
        b: 0,
        a: 255,
    };
    image.set(0, 1, left);
    image.set(1, 1, middle);
    image.set(2, 1, right);

    let rotated = rotate_nearest(&image, 90.0);

    assert_eq!((rotated.width, rotated.height), (3, 3));
    assert_eq!(rotated.get(1, 0), right);
    assert_eq!(rotated.get(1, 1), middle);
    assert_eq!(rotated.get(1, 2), left);
}

#[test]
fn structural_layer_keeps_red_edges_but_not_red_interiors() {
    let mut image = Image::transparent(100, 100);
    let dark_red = Pixel {
        r: 96,
        g: 0,
        b: 0,
        a: 255,
    };
    for y in 20..25 {
        for x in 50..55 {
            image.set(x, y, dark_red);
        }
    }

    let structural = structural_layer(
        &image,
        Bounds {
            x: 0,
            y: 0,
            width: image.width,
            height: image.height,
        },
    );
    assert_eq!(structural[20 * image.width + 50], 1);
    assert_eq!(structural[22 * image.width + 52], 0);
}

#[test]
fn structural_layer_clears_entire_badge_window() {
    let mut image = Image::transparent(100, 40);
    let gold = Pixel {
        r: 255,
        g: 192,
        b: 0,
        a: 255,
    };
    let dark_red = Pixel {
        r: 96,
        g: 0,
        b: 0,
        a: 255,
    };
    image.set(18, 35, dark_red);
    image.set(29, 35, gold);

    let structural = structural_layer(
        &image,
        Bounds {
            x: 0,
            y: 0,
            width: image.width,
            height: image.height,
        },
    );
    assert_eq!(structural[35 * image.width + 18], 0);
    assert_eq!(structural[35 * image.width + 29], 0);

    for y in 29..40 {
        for x in 18..31 {
            assert_eq!(structural[y * image.width + x], 0);
        }
    }
}

#[test]
fn structural_layer_clears_lowest_badge_fragment_without_cutting_glyph_edge() {
    let mut image = Image::transparent(160, 62);
    let dark_red = Pixel {
        r: 96,
        g: 0,
        b: 0,
        a: 255,
    };
    image.set(67, 45, dark_red);
    image.set(50, 45, dark_red);

    let structural = structural_layer(
        &image,
        Bounds {
            x: 0,
            y: 0,
            width: image.width,
            height: image.height,
        },
    );

    assert_eq!(structural[45 * image.width + 67], 0);
    assert_eq!(structural[45 * image.width + 50], 1);
}

#[test]
fn subtitle_shear_alternates_backslash_and_slash() {
    let white = Pixel {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    let mut line = Image::transparent(1, 5);
    for y in 0..5 {
        line.set(0, y, white);
    }

    let backslash = shear_glyph(&line, GlyphLean::Backslash, 4);
    let slash = shear_glyph(&line, GlyphLean::Slash, 4);
    assert_eq!(backslash.get(0, 0), white);
    assert_eq!(backslash.get(4, 4), white);
    assert_eq!(slash.get(4, 0), white);
    assert_eq!(slash.get(0, 4), white);
}

#[test]
fn subtitle_layout_is_level_tight_and_fits_both_surfaces() {
    assert_eq!(subtitle_shear(0), SUBTITLE_BASE_SHEAR);
    assert_eq!(subtitle_shear(1), SUBTITLE_YU_SHEAR);
    assert_eq!(subtitle_shear(2), SUBTITLE_BASE_SHEAR);
    assert_eq!(subtitle_shear(3), SUBTITLE_WON_SHEAR);
    assert_eq!(subtitle_shear(4), SUBTITLE_A_SHEAR);
    assert_eq!(DAE_BODY_WIDTH + subtitle_shear(0), 29);
    assert!(DAE_TOP + SUBTITLE_GLYPH_HEIGHT <= 64);
    assert_eq!(
        rest_glyph_body_size(1),
        (REST_YU_BODY_WIDTH, REST_EMPHASIZED_BODY_HEIGHT)
    );
    assert_eq!(rest_glyph_body_size(2), (REST_BODY_WIDTH, REST_BODY_HEIGHT));
    assert_eq!(
        rest_glyph_body_size(3),
        (REST_EMPHASIZED_BODY_WIDTH, REST_EMPHASIZED_BODY_HEIGHT)
    );
    assert_eq!(
        rest_glyph_body_size(4),
        (REST_EMPHASIZED_BODY_WIDTH, REST_EMPHASIZED_BODY_HEIGHT)
    );
    assert_eq!(
        REST_GLYPH_LEFTS[3] + REST_EMPHASIZED_BODY_WIDTH + subtitle_shear(4),
        REST_CANVAS_WIDTH
    );
    assert!(REST_BASELINE <= REST_CANVAS_HEIGHT);
    for index in 0..3 {
        let subtitle_index = index + 1;
        let (body_width, _) = rest_glyph_body_size(subtitle_index);
        assert!(
            REST_GLYPH_LEFTS[index + 1]
                < REST_GLYPH_LEFTS[index] + body_width + subtitle_shear(subtitle_index)
        );
    }
}

#[test]
fn mode7_writer_uses_runtime_tile_order() {
    let mut indexed = vec![0u8; 256 * 64];
    let points = [(0, 0), (17, 9), (255, 63)];
    for &(x, y) in &points {
        indexed[y * 256 + x] = 3;
    }

    let mut chr = vec![0u8; 256 * 64];
    write_mode7_tiles(&mut chr, &indexed).unwrap();

    assert_eq!(
        chr.iter().filter(|&&value| value != 0).count(),
        points.len()
    );
    for &(x, y) in &points {
        let tile = (y / 8) * 32 + x / 8;
        let offset = tile * 64 + (y % 8) * 8 + x % 8;
        assert_eq!(chr[offset], 1);
    }
}
