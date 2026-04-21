const {invoke} = window.__TAURI__.core;
const {getCurrentWindow} = window.__TAURI__.window;

let settings = null;

window.onload = function () {
  document.addEventListener('contextmenu', (event) => {
    event.preventDefault();
  });

  setStaticUiEvents();

  /* メインプロセスにデータ取得要求 */
  invoke("cmd_get_local_setting", {}) // get_local_settingと同じため流用
    .then((message) => {
      console.log("OK");
      console.log(message);
      settings = message;
      document.getElementById('now_encoding_label').innerHTML = message.encoding;
      document.getElementById('new_encoding_selector').value = message.encoding;
    })
    .catch((error) => {
      console.log("Err");
    });
};

function setStaticUiEvents () {
  const cancelButton = document.getElementById('cancel_button');
  const okButton = document.getElementById('ok_button');

  cancelButton.addEventListener('click', cancel);
  okButton.addEventListener('click', reloadEncoding);
}

// HTMLから呼び出すためLintは無効化
// eslint-disable-next-line no-unused-vars
function reloadEncoding () {
  /* 設定をセットし、メインプロセスに送信 */
  settings.new_encoding = document.getElementById('new_encoding_selector').value;
  // window.api.reloadEncoding(settings);
  invoke("cmd_reload_encoding", {payload: settings});
}

// HTMLから呼び出すためLintは無効化
// eslint-disable-next-line no-unused-vars
function cancel () {
  getCurrentWindow().close();
}
