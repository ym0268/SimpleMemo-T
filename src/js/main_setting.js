const {invoke} = window.__TAURI__.core;
const {getCurrentWindow} = window.__TAURI__.window;

let settings = null;

/* 全体設定ウィンドウ */
window.onload = function () {
  document.addEventListener('contextmenu', (event) => {
    event.preventDefault();
  });

  setStaticUiEvents();
  setDefaultValues();
  /* メインプロセスにデータ取得要求 */
  // window.api.getSetting();
  invoke("cmd_get_global_setting", {})
    .then((message) => {
      console.log("OK");
      console.log(message);
      settings = message;  // グローバルにセット（ここで設定しない値もあるため）

      /* 基本設定 */
      document.getElementById('savepath_textbox').value = message.savepath;
      document.getElementById('font_selector').value = message.font;
      document.getElementById('fontsize_textbox').value = message.fontsize;
      document.getElementById('encoding_selector').value = message.encoding;
      document.getElementById('autoencoding_checkbox').checked = message.auto_encoding;
      document.getElementById('topmost_checkbox').checked = message.top_most;

      /* 高度な設定(未実装) */
      // document.getElementById('load_lastfile_checkbox').checked = message.load_last_file;
      // document.getElementById('no_close_dialog_checkbox').checked = message.no_close_dialog;
      // document.getElementById('autosave_checkbox').checked = message.auto_save;
      // document.getElementById('autosave_input').value = message.auto_save_span;
      // document.getElementById('autolock_checkbox').value = message.auto_lock;
    })
    .catch((error) => {
      console.log("[window.onload] failed to invoke 'cmd_get_global_setting': \n", error);
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
  console.log('set');

  /* 基本設定 */
  settings.savepath = document.getElementById('savepath_textbox').value;
  settings.font = document.getElementById('font_selector').value;
  settings.fontsize = parseInt(document.getElementById('fontsize_textbox').value);
  settings.encoding = document.getElementById('encoding_selector').value;
  settings.auto_encoding = document.getElementById('autoencoding_checkbox').checked;
  settings.top_most = document.getElementById('topmost_checkbox').checked;

  console.log(settings);

  /* 高度な設定 */
  // TODO

  // 設定を送信
  // window.api.setSetting(settings);
  invoke("cmd_set_global_setting", {payload: settings})
    .catch((error) => {
      console.log("[setSetting] failed to invoke 'cmd_set_global_setting': \n", error);
      /* TODO: フォントサイズがu32以外の場合のエラー処理。エラーメッセージを表示するか、変更前の値に置換するなどの対応を入れる */
      alert("設定が正しくありません");
    });
}

// HTMLから呼び出すためLintは無効化
// eslint-disable-next-line no-unused-vars
function cancel () {
  // TODO: バックエンドで閉じるようにしたい（OKボタンを押したときはバックエンドで閉じるため）
  getCurrentWindow().close();
}

/**
 * 初期値をセットする
 */
function setDefaultValues () {
}
