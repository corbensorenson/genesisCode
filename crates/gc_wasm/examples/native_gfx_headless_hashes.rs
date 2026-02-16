use gc_coreform::parse_term;

fn hex_lower(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

fn main() {
    // Keep this source exactly aligned with scripts/wasm_web_smoke.mjs checks.
    let frame_graph_src = r#"
      {
        :type :gfx/frame-graph
        :render-passes [
          {
            :type :gfx/render-pass
            :label "web-golden"
            :commands [
              {
                :op :set-pipeline
                :pipeline 1
              }
              {
                :op :draw
                :vertex-count 3
                :instance-count 1
                :first-vertex 0
                :first-instance 0
              }
            ]
          }
        ]
        :compute-passes []
      }
    "#;
    let frame = parse_term(frame_graph_src).expect("frame graph parse");
    let img = gc_gfx::render_frame_graph_headless(&frame, 160, 90).expect("headless render");

    let out = format!(
        concat!(
            "{{",
            "\"gfx_pixel_h\":\"{}\",",
            "\"gfx_png_h\":\"{}\"",
            "}}"
        ),
        hex_lower(&img.pixel_hash),
        hex_lower(&img.png_hash),
    );
    print!("{out}");
}
