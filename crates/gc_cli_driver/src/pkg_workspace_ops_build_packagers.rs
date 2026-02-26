use super::*;
use std::borrow::Cow;
use wasm_encoder::{
    CodeSection, CustomSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
    Module, TypeSection,
};

pub(super) struct TargetPackagePayloadInput<'a> {
    pub(super) target_label: &'a str,
    pub(super) bundle_h: &'a str,
    pub(super) target_profile: &'a BuildTargetProfile,
    pub(super) package_h: &'a str,
    pub(super) package_artifact: &'a str,
    pub(super) entrypoint_src: &'a str,
    pub(super) entrypoint_h: &'a str,
}

pub(super) struct TargetPackagePayload {
    pub(super) bytes: Vec<u8>,
    pub(super) payload_kind: &'static str,
}

pub(super) fn build_target_package_payload(
    input: &TargetPackagePayloadInput<'_>,
) -> Result<TargetPackagePayload, String> {
    match input.target_label {
        "ios" => build_ios_ipa_payload(input),
        "android" => build_android_aab_payload(input),
        "edge" => build_wasm_runtime_payload(input, false),
        "service-runtime" => build_wasm_runtime_payload(input, true),
        _ => build_coreform_target_payload(input),
    }
}

fn build_coreform_target_payload(
    input: &TargetPackagePayloadInput<'_>,
) -> Result<TargetPackagePayload, String> {
    let package_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/target-package"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(input.target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-format")),
                Term::Str(input.target_profile.artifact_format.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime")),
                Term::Str(input.target_profile.runtime.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":host-profile")),
                Term::Str(input.target_profile.host_profile.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":bundle-h")),
                Term::Str(input.bundle_h.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package-h")),
                Term::Str(input.package_h.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(input.package_artifact.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":execution-lanes")),
                proper_list(vec![Term::symbol(":boot"), Term::symbol(":smoke")]),
            ),
            (
                TermOrdKey(Term::symbol(":entrypoint")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str("entrypoint.gc".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(input.entrypoint_h.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(TargetPackagePayload {
        bytes: (gc_coreform::print_term(&package_term) + "\n").into_bytes(),
        payload_kind: "coreform-map-v1",
    })
}

fn build_ios_ipa_payload(
    input: &TargetPackagePayloadInput<'_>,
) -> Result<TargetPackagePayload, String> {
    let mut zip = DeterministicZip::default();
    let info_plist = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\"><dict>",
            "<key>CFBundleIdentifier</key><string>org.genesiscode.app</string>",
            "<key>CFBundleName</key><string>GenesisCode</string>",
            "<key>GenesisBundleHash</key><string>{}</string>",
            "<key>GenesisEntryHash</key><string>{}</string>",
            "</dict></plist>\n"
        ),
        input.bundle_h, input.entrypoint_h
    );
    zip.push_file("Payload/Genesis.app/Info.plist", info_plist.as_bytes())?;
    zip.push_file(
        "Payload/Genesis.app/entrypoint.gc",
        input.entrypoint_src.as_bytes(),
    )?;
    let runtime_gc = build_runtime_descriptor_gc(input);
    zip.push_file("Payload/Genesis.app/runtime.gc", runtime_gc.as_bytes())?;
    Ok(TargetPackagePayload {
        bytes: zip.finish()?,
        payload_kind: "ios-ipa-zip-v1",
    })
}

fn build_android_aab_payload(
    input: &TargetPackagePayloadInput<'_>,
) -> Result<TargetPackagePayload, String> {
    let mut zip = DeterministicZip::default();
    let android_manifest = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
            "<manifest package=\"org.genesiscode.app\" ",
            "xmlns:android=\"http://schemas.android.com/apk/res/android\">",
            "<application android:label=\"GenesisCode\" ",
            "android:hasCode=\"false\">",
            "<meta-data android:name=\"genesis.bundle_h\" android:value=\"{}\"/>",
            "<meta-data android:name=\"genesis.entry_h\" android:value=\"{}\"/>",
            "</application></manifest>\n"
        ),
        input.bundle_h, input.entrypoint_h
    );
    zip.push_file(
        "base/manifest/AndroidManifest.xml",
        android_manifest.as_bytes(),
    )?;
    zip.push_file("base/assets/entrypoint.gc", input.entrypoint_src.as_bytes())?;
    let runtime_gc = build_runtime_descriptor_gc(input);
    zip.push_file(
        "BUNDLE-METADATA/com.genesis/runtime.gc",
        runtime_gc.as_bytes(),
    )?;
    Ok(TargetPackagePayload {
        bytes: zip.finish()?,
        payload_kind: "android-aab-zip-v1",
    })
}

fn build_wasm_runtime_payload(
    input: &TargetPackagePayloadInput<'_>,
    is_service_runtime: bool,
) -> Result<TargetPackagePayload, String> {
    let mut module = Module::new();
    let meta_sections = [
        ("genesis.target", input.target_label),
        ("genesis.bundle_h", input.bundle_h),
        ("genesis.package_h", input.package_h),
        ("genesis.package_artifact", input.package_artifact),
        ("genesis.entry_h", input.entrypoint_h),
    ];
    for (name, value) in meta_sections {
        module.section(&CustomSection {
            name: Cow::Borrowed(name),
            data: Cow::Owned(value.as_bytes().to_vec()),
        });
    }

    let mut types = TypeSection::new();
    types.ty().function([], []);
    module.section(&types);

    let mut funcs = FunctionSection::new();
    funcs.function(0);
    module.section(&funcs);

    let export_name = if is_service_runtime {
        "_start"
    } else {
        "handle"
    };
    let mut exports = ExportSection::new();
    exports.export(export_name, ExportKind::Func, 0);
    module.section(&exports);

    let mut code = CodeSection::new();
    let mut body = Function::new(Vec::new());
    body.instruction(&Instruction::End);
    code.function(&body);
    module.section(&code);

    Ok(TargetPackagePayload {
        bytes: module.finish(),
        payload_kind: if is_service_runtime {
            "service-runtime-wasm-module-v1"
        } else {
            "edge-wasm-module-v1"
        },
    })
}

fn build_runtime_descriptor_gc(input: &TargetPackagePayloadInput<'_>) -> String {
    let descriptor = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/runtime"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(input.target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":bundle-h")),
                Term::Str(input.bundle_h.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime")),
                Term::Str(input.target_profile.runtime.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":host-profile")),
                Term::Str(input.target_profile.host_profile.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":entrypoint-h")),
                Term::Str(input.entrypoint_h.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    gc_coreform::print_term(&descriptor) + "\n"
}

#[derive(Default)]
struct DeterministicZip {
    entries: Vec<ZipEntry>,
}

struct ZipEntry {
    name: String,
    crc32: u32,
    bytes: Vec<u8>,
}

impl DeterministicZip {
    fn push_file(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if name.is_empty() {
            return Err("zip entry name must not be empty".to_string());
        }
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("duplicate zip entry `{name}`"));
        }
        self.entries.push(ZipEntry {
            name: name.to_string(),
            crc32: crc32(bytes),
            bytes: bytes.to_vec(),
        });
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>, String> {
        self.entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut out = Vec::new();
        let mut central = Vec::new();
        let mut offset: u32 = 0;

        for entry in &self.entries {
            let name = entry.name.as_bytes();
            let size = u32::try_from(entry.bytes.len())
                .map_err(|_| format!("zip entry `{}` exceeds u32 size", entry.name))?;
            let name_len = u16::try_from(name.len())
                .map_err(|_| format!("zip entry `{}` name is too long", entry.name))?;

            write_u32_le(&mut out, 0x0403_4b50);
            write_u16_le(&mut out, 20);
            write_u16_le(&mut out, 0);
            write_u16_le(&mut out, 0);
            write_u16_le(&mut out, 0);
            write_u16_le(&mut out, 0);
            write_u32_le(&mut out, entry.crc32);
            write_u32_le(&mut out, size);
            write_u32_le(&mut out, size);
            write_u16_le(&mut out, name_len);
            write_u16_le(&mut out, 0);
            out.extend_from_slice(name);
            out.extend_from_slice(&entry.bytes);

            write_u32_le(&mut central, 0x0201_4b50);
            write_u16_le(&mut central, 20);
            write_u16_le(&mut central, 20);
            write_u16_le(&mut central, 0);
            write_u16_le(&mut central, 0);
            write_u16_le(&mut central, 0);
            write_u16_le(&mut central, 0);
            write_u32_le(&mut central, entry.crc32);
            write_u32_le(&mut central, size);
            write_u32_le(&mut central, size);
            write_u16_le(&mut central, name_len);
            write_u16_le(&mut central, 0);
            write_u16_le(&mut central, 0);
            write_u16_le(&mut central, 0);
            write_u16_le(&mut central, 0);
            write_u32_le(&mut central, 0);
            write_u32_le(&mut central, offset);
            central.extend_from_slice(name);

            let local_header_len = 30u32;
            offset = offset
                .checked_add(local_header_len)
                .and_then(|x| x.checked_add(u32::from(name_len)))
                .and_then(|x| x.checked_add(size))
                .ok_or_else(|| "zip archive size overflow".to_string())?;
        }

        let central_offset =
            u32::try_from(out.len()).map_err(|_| "zip archive exceeds u32 size".to_string())?;
        out.extend_from_slice(&central);
        let central_size = u32::try_from(central.len())
            .map_err(|_| "zip central directory exceeds u32 size".to_string())?;
        let entry_count = u16::try_from(self.entries.len())
            .map_err(|_| "zip has too many entries".to_string())?;

        write_u32_le(&mut out, 0x0605_4b50);
        write_u16_le(&mut out, 0);
        write_u16_le(&mut out, 0);
        write_u16_le(&mut out, entry_count);
        write_u16_le(&mut out, entry_count);
        write_u32_le(&mut out, central_size);
        write_u32_le(&mut out, central_offset);
        write_u16_le(&mut out, 0);

        Ok(out)
    }
}

fn write_u16_le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let lsb = crc & 1;
            crc >>= 1;
            if lsb != 0 {
                crc ^= 0xedb8_8320;
            }
        }
    }
    !crc
}
