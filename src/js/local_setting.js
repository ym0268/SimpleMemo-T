const {invoke} = window.__TAURI__.core;
const {getCurrentWindow} = window.__TAURI__.window;

let settings = null;

window.onload = function () {
  document.addEventListener('contextmenu', (event) => {
    event.preventDefault();
  });

  setStaticUiEvents();

  /* メインプロセスにデータ取得要求 */
  invoke("cmd_get_local_setting", {})
    .then((message) => {
      console.log("OK");
      console.log(message);
      settings = message;

      /* 基本設定 */
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
  okButton.addEventListener('click', setSetting);
}

// HTMLから呼び出すためLintは無効化
// eslint-disable-next-line no-unused-vars
function setSetting () {
  /* 設定をセットし、メインプロセスに送信 */
  settings.encoding = document.getElementById('new_encoding_selector').value;
  invoke("cmd_set_local_setting", {payload: settings});
}

// HTMLから呼び出すためLintは無効化
// eslint-disable-next-line no-unused-vars
function cancel () {
  getCurrentWindow().close();
}
