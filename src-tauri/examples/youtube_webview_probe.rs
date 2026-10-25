//! Manual native WKWebView / WebView2 probe of the production loopback bridge.
//! Run explicitly: cargo run --example youtube_webview_probe. Opens ONE window.
//! No sign-in automation or ordinary browser/profile access. Use the official
//! player's own play button; controls below exercise documented API operations.
use movie_party_lib::providers::youtube::embed::EmbedServer;
use tauri::Manager;
#[tauri::command]
fn probe_event(value: serde_json::Value) {
    println!("YOUTUBE_NATIVE {}", value);
}
fn main() {
    let server = EmbedServer::start().expect("loopback player");
    let page = format!("{}#video=M7lc1UVf-VE&lease=native-probe", server.page);
    let script = format!(
        r#"window.addEventListener('DOMContentLoaded',()=>{{
 const iframe=document.createElement('iframe');let url=new URL({page});url.hash+='&parent='+encodeURIComponent(location.origin);iframe.src=url.href;iframe.allow='autoplay; encrypted-media; fullscreen';iframe.style='position:fixed;left:24px;top:24px;width:calc(100vw - 48px);height:calc(100vh - 140px);border:0;z-index:99999';document.body.appendChild(iframe);
 let serial=0,last='',seen=false;const origin=new URL(url).origin;
 const send=data=>iframe.contentWindow.postMessage({{...data,lease:'native-probe'}},origin);
 const controls=document.createElement('div');controls.style='position:fixed;left:24px;bottom:24px;z-index:99999;display:flex;gap:16px;color:white';
 for(const action of ['PLAY','PAUSE','SEEK','DISPOSE']){{const button=document.createElement('button');button.textContent=action;button.style='padding:12px;background:#302244;color:white';button.onclick=()=>{{if(action==='DISPOSE'){{send({{type:'dispose'}});iframe.remove();}}else send({{type:'command',serial:++serial,action,target_ms:action==='SEEK'?10000:0,expires_at_ms:Date.now()+500}});}};controls.appendChild(button);}}document.body.appendChild(controls);
 window.addEventListener('message',event=>{{if(event.source!==iframe.contentWindow||event.origin!==origin||event.data.lease!=='native-probe')return;const data=event.data;send({{type:'visible',visible:true}});const value={{videoId:data.videoId,ready:data.playable,paused:data.player?.paused,position:Math.floor((data.player?.position_ms||0)/1000),error:data.error,result:data.result,channel:data.metadata?.channel}};const key=JSON.stringify(value);if(last!==key){{last=key;window.__TAURI_INTERNALS__.invoke('probe_event',{{value}});}}seen=true;}});
 setTimeout(()=>{{if(!seen)window.__TAURI_INTERNALS__.invoke('probe_event',{{value:{{error:'NO_BRIDGE_EVENTS'}}}});}},20000);
 }});"#,
        page = serde_json::to_string(&page).unwrap()
    );
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![probe_event])
        .setup(move |app| {
            app.manage(server);
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Movie Party — native YouTube validation")
            .inner_size(1024., 700.)
            .initialization_script(&script)
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!(
            "examples/youtube-probe/tauri.conf.json"
        ))
        .expect("native WebView");
}
