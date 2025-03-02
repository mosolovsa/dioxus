use dioxus_interpreter_js::INTERPRETER_JS;
use std::path::{Path, PathBuf};
use wry::{
    http::{status::StatusCode, Request, Response},
    Result,
};

fn module_loader(root_name: &str) -> String {
    format!(
        r#"
<script>
    {INTERPRETER_JS}

    let rootname = "{}";
    let root = window.document.getElementById(rootname);
    if (root != null) {{
        window.interpreter = new Interpreter(root);
        window.ipc.postMessage(serializeIpcMessage("initialize"));
    }}
</script>
"#,
        root_name
    )
}

pub(super) fn desktop_handler(
    request: &Request<Vec<u8>>,
    asset_root: Option<PathBuf>,
    custom_head: Option<String>,
    custom_index: Option<String>,
    root_name: &str,
) -> Result<Response<Vec<u8>>> {
    // Any content that uses the `dioxus://` scheme will be shuttled through this handler as a "special case".
    // For now, we only serve two pieces of content which get included as bytes into the final binary.
    let path = request.uri().to_string().replace("dioxus://", "");

    // all assets should be called from index.html
    let trimmed = path.trim_start_matches("index.html/");

    if trimmed.is_empty() {
        // If a custom index is provided, just defer to that, expecting the user to know what they're doing.
        // we'll look for the closing </body> tag and insert our little module loader there.
        if let Some(custom_index) = custom_index {
            let rendered = custom_index
                .replace("</body>", &format!("{}</body>", module_loader(root_name)))
                .into_bytes();
            Response::builder()
                .header("Content-Type", "text/html")
                .body(rendered)
                .map_err(From::from)
        } else {
            // Otherwise, we'll serve the default index.html and apply a custom head if that's specified.
            let mut template = include_str!("./index.html").to_string();
            if let Some(custom_head) = custom_head {
                template = template.replace("<!-- CUSTOM HEAD -->", &custom_head);
            }
            template = template.replace("<!-- MODULE LOADER -->", &module_loader(root_name));

            Response::builder()
                .header("Content-Type", "text/html")
                .body(template.into_bytes())
                .map_err(From::from)
        }
    } else if trimmed == "index.js" {
        Response::builder()
            .header("Content-Type", "text/javascript")
            .body(dioxus_interpreter_js::INTERPRETER_JS.as_bytes().to_vec())
            .map_err(From::from)
    } else {
        let asset_root = asset_root
            .unwrap_or_else(|| get_asset_root().unwrap_or_else(|| Path::new(".").to_path_buf()))
            .canonicalize()?;

        let asset = asset_root.join(trimmed).canonicalize()?;

        if !asset.starts_with(asset_root) {
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(String::from("Forbidden").into_bytes())
                .map_err(From::from);
        }

        if !asset.exists() {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(String::from("Not Found").into_bytes())
                .map_err(From::from);
        }

        Response::builder()
            .header("Content-Type", get_mime_from_path(trimmed)?)
            .body(std::fs::read(asset)?)
            .map_err(From::from)
    }
}

#[allow(unreachable_code)]
fn get_asset_root() -> Option<PathBuf> {
    /*
    We're matching exactly how cargo-bundle works.

    - [x] macOS
    - [ ] Windows
    - [ ] Linux (rpm)
    - [ ] Linux (deb)
    - [ ] iOS
    - [ ] Android

    */

    if std::env::var_os("CARGO").is_some() {
        return None;
    }

    // TODO: support for other platforms
    #[cfg(target_os = "macos")]
    {
        let bundle = core_foundation::bundle::CFBundle::main_bundle();
        let bundle_path = bundle.path()?;
        let resources_path = bundle.resources_path()?;
        let absolute_resources_root = bundle_path.join(resources_path);
        let canonical_resources_root = dunce::canonicalize(absolute_resources_root).ok()?;

        return Some(canonical_resources_root);
    }

    None
}

/// Get the mime type from a path-like string
fn get_mime_from_path(trimmed: &str) -> Result<&str> {
    if trimmed.ends_with(".svg") {
        return Ok("image/svg+xml");
    }

    let res = match infer::get_from_path(trimmed)?.map(|f| f.mime_type()) {
        Some(t) if t == "text/plain" => get_mime_by_ext(trimmed),
        Some(f) => f,
        None => get_mime_by_ext(trimmed),
    };

    Ok(res)
}

/// Get the mime type from a URI using its extension
fn get_mime_by_ext(trimmed: &str) -> &str {
    let suffix = trimmed.split('.').last();
    match suffix {
        Some("bin") => "application/octet-stream",
        Some("css") => "text/css",
        Some("csv") => "text/csv",
        Some("html") => "text/html",
        Some("ico") => "image/vnd.microsoft.icon",
        Some("js") => "text/javascript",
        Some("json") => "application/json",
        Some("jsonld") => "application/ld+json",
        Some("mjs") => "text/javascript",
        Some("rtf") => "application/rtf",
        Some("svg") => "image/svg+xml",
        Some("mp4") => "video/mp4",
        // Assume HTML when a TLD is found for eg. `dioxus:://dioxuslabs.app` | `dioxus://hello.com`
        Some(_) => "text/html",
        // https://developer.mozilla.org/en-US/docs/Web/HTTP/Basics_of_HTTP/MIME_types/Common_types
        // using octet stream according to this:
        None => "application/octet-stream",
    }
}
