/* global Mousetrap */
const {invoke} = window.__TAURI__.core;
const {listen, TauriEvent} = window.__TAURI__.event;

/* メインウィンドウ */
const WINDOW_TITLE = 'SimpleMemo';
// TODO: ウィンドウタイトルを取得するようにしたい
// let WINDOW_TITLE = "";
// window.__TAURI__.window.getCurrentWindow().title().then((title) => {
//   WINDOW_TITLE = title;
// });
let nowPage = 0;
const MAX_PAGENUM = 3;
let fontSize = 16;
let unsaveList = null;
// let scrollPosList = null;     /* 未使用 */
let saveNotificationId = null;
let lockStatus = null;

// TODO: TauriEvent.WINDOW_CREATEDにした方がよい？
window.onload = async function () {
  setStaticUiEvents();
  setContextMenu();
  setKeyBind();
  // setDragAndDrop();
  // setFontSize(fontSize);
  enableTabkey();
  enableDetectChange();
  unsaveList = [...Array(MAX_PAGENUM).keys()].map(() => { return false; });    // 未保存リストをfalseで初期化
  // scrollPosList = [...Array(MAX_PAGENUM).keys()].map((d) => { return 0; });
  lockStatus = [...Array(MAX_PAGENUM).keys()].map(() => { return false; });
  document.getElementById('textarea_0').focus();  // 最初の面にフォーカス
  document.title = WINDOW_TITLE;

  // イベントリスナー登録
  await registerEventListener();

  // メインプロセスにウィンドウロード完了を通知
  invoke("cmd_main_window_ready", {})
    .catch((error) => {
      console.log("[window.onload] failed to invoke 'cmd_main_window_ready': \n", error);
    });

  // UI設定を取得・反映
  // TODO: 以下はcmd_main_window_ready対応により不要なはずなので、削除予定
  // invoke("cmd_get_global_setting", {})
  //   .then((message) => {
  //     console.log("cmd_get_global_setting OK");
  //     console.log(message);
  //     setUiSetting(message.fontsize, message.font);
  //   })
  //   .catch((error) => {
  //     console.log("cmd_get_global_setting Err");
  //     console.log(error);
  //   });

  // コンテキストメニューを抑止
  document.addEventListener('contextmenu', (event) => event.preventDefault());
};

function setStaticUiEvents () {
  const sheetButton = document.getElementById('sheet_button');
  sheetButton.addEventListener('click', nextPage);
  sheetButton.addEventListener('contextmenu', (event) => {
    event.preventDefault();
    prevPage();
  });

  const saveButton = document.getElementById('save_button');
  saveButton.addEventListener('click', saveFile);
}

/**
 * 次の面に遷移する
 */
function nextPage () {
  stopSaveNotification();
  nowPage++;
  if (nowPage >= MAX_PAGENUM) {
    nowPage = 0;
  }
  setPage(nowPage);
}

/**
 * 前の面に遷移する
 */
function prevPage () {
  stopSaveNotification();
  nowPage--;
  if (nowPage < 0) {
    nowPage = MAX_PAGENUM - 1;
  }
  setPage(nowPage);
}

/**
 * テキスト領域の要素を取得する
 * @param {Number} pageNum
 * @returns テキスト領域
 */
function getTextarea (pageNum) {
  const id = 'textarea_' + pageNum.toString();
  return document.getElementById(id);
}

/**
 * ファイル名テキストボックスの要素を取得する
 * @param {Number} pageNum
 * @returns ファイル名テキストボックス
 */
function getFilenameTextbox (pageNum) {
  const id = 'filename_textbox_' + pageNum.toString();
  return document.getElementById(id);
}

/**
 * 表示する面をセットする
 * @param {Number} pagenum セットしたい面番号(0始まり)
 *
 */
function setPage (pagenum) {
  for (let i = 0; i < MAX_PAGENUM; i++) {
    const txtarea = getTextarea(i);
    const fnameTextbox = getFilenameTextbox(i);
    if (pagenum === i) {
      txtarea.style.display = 'block';
      fnameTextbox.style.display = 'block';
      txtarea.focus({ preventScroll: true });      // フォーカスを合わせる
    } else {
      txtarea.style.display = 'none';
      fnameTextbox.style.display = 'none';
    }
  }
  // ボタンの番号を更新
  document.getElementById('sheet_button').innerHTML = (nowPage + 1).toString();
  // メインプロセスに変更を通知
  invoke("cmd_set_pagenum", {pageNum: nowPage});
  // ロック状態を更新
  updateLockStatus();
  // 未保存表示を更新
  updateUnsavedStatus(unsaveList[nowPage]); // 現在表示している面(nowPage)の未保存表示を更新
}

/**
 * コンテキストメニューをテキスト入力領域にセットする
 * 画面ロード時に呼び出す
 */
function setContextMenu () {
  for (let i = 0; i < MAX_PAGENUM; i++) {
    const txtarea = getTextarea(i);
    txtarea.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      invoke("cmd_show_context_menu", {});
    });
  }
}

/**
 * ファイルを保存する
 */
function saveFile () {
  console.log("saveFile()");
  // ロック中ならファイル保存しない
  if (lockStatus[nowPage]) {
    return;
  }
  const txtarea = getTextarea(nowPage);
  const fnameTextbox = getFilenameTextbox(nowPage);
  const data = {
    page_num: nowPage,
    filename: fnameTextbox.value,
    text: txtarea.value,
  };
  console.log(data);
  invoke("cmd_save_file", {payload: data})
    .then((message) => {
      if (message.is_external_file === true) {
        // 外部ファイル保存
        console.log('external file');
        startSaveNotification('LightSalmon');
        unsaveList[message.page_num] = false;
      } else if(message.save_count === 1) {
        // 初回保存
        console.log('first save');
        startSaveNotification('Aqua');
        unsaveList[message.page_num] = false;
      } else {
        // 上書き保存
        console.log('overwrite');
        startSaveNotification('GreenYellow');
        unsaveList[message.page_num] = false;
      }
      // 未保存表示更新
      updateUnsavedStatus(unsaveList[nowPage]);
    })
    .catch((error) => {
      // エラー発生
      console.log('error:');
      console.log(error);
      // startSaveNotification('red');
    });
}

/**
 * 保存完了通知を開始する
 * ファイル名テキストボックスの色を変更する
 * @param {String} color
 */
function startSaveNotification (color) {
  const fnameTextbox = getFilenameTextbox(nowPage);
  fnameTextbox.style.backgroundColor = color;
  saveNotificationId = window.setTimeout(stopSaveNotification, 2000);
}

/**
 * 保存完了通知を終了する
 * タイマ呼び出しを想定
 */
function stopSaveNotification () {
  if (saveNotificationId !== null) {
    clearTimeout(saveNotificationId);
    saveNotificationId = null;
  }
  const fnameTextbox = getFilenameTextbox(nowPage);
  fnameTextbox.style.backgroundColor = 'transparent';
}

// /**
//  * キーを解析し、目的のキーが入力されたかを判定する
//  * デフォルトはオプションキーはFalse
//  * @param {KeyboardEvent} event
//  * @param {Boolean} targetKey
//  * @param {Boolean} ctrlKey
//  * @param {Boolean} shiftKey
//  * @param {Boolean} metakey
//  * @param {Boolean} isComposing
//  *
//  * @return {Boolean} キーが一致したか
//  */
// function keyParser(event, targetKey, ctrlKey=false, shiftKey=false, isComposing=false){
//     return (event.key==targetKey && ctrlKey==ctrlKey && shiftKey==shiftKey && isComposing==isComposing);
// }

/**
 * フォントサイズを指定する（px）
 * @param {Number} size
 */
function setFontSize (size) {
  if (size > 0) {
    for (let i = 0; i < MAX_PAGENUM; i++) {
      const txtarea = getTextarea(i);
      txtarea.style.fontSize = size.toString() + 'px';
      console.log('size=' + size.toString());
    }
    fontSize = size;

    // フォントサイズをメインプロセスに通知
    invoke("cmd_set_fontsize", {fontsize: fontSize});
  }
}

/**
 * ファイル名テキストボックスとテキストエリアのフォーカスを切り替える
 */
function switchTextboxFocus () {
  const txtarea = getTextarea(nowPage);
  const fnameTextbox = getFilenameTextbox(nowPage);
  if (document.activeElement === txtarea) {
    fnameTextbox.focus({ preventScroll: true });
  } else {
    txtarea.focus({ preventScroll: true });
  }
}

/**
 * ロック状態を更新し、メインプロセスに通知する
 * UI側からロック状態を更新することを想定（ショートカットキーなど）
 */
function setLockStatus (pageNum) {
  /* ロック状態をトグル */
  lockStatus[pageNum] = !lockStatus[pageNum];
  invoke("cmd_set_lock_status_main", {pageNum: pageNum});
}

/**
 * ロック状態を更新する
 * 面切り替え時に呼び出す
 */
function updateLockStatus () {
  updateLockStatusUI();
  invoke("cmd_update_lock_status_main", {});
}

/**
 * 未保存表示を更新する
 * @param {Boolean} unsaved 未保存ならtrue
 * @note
 * 未保存の場合、ウィンドウ名に'*'をつける
 */
function updateUnsavedStatus (unsaved) {
  let title = WINDOW_TITLE;
  if (unsaved === true) {
    title = '*' + title;
  }
  document.title = title;
  const appWindow = window.__TAURI__.window.getCurrentWindow();
  appWindow.setTitle(title);
}

/**
 * 指定したページの文字数をカウントする
 * @param {Number} pageNum 
 * @returns 文字数
 * @note 改行コードを含む
 */
function countCharacters (pageNum) {
  let l = 0;
  const txtarea = getTextarea(pageNum);
  if(txtarea != null){
    l = txtarea.value.length;
  } else {
    // textarea取得失敗
    l = -1;
  }
  console.log("[countCharacters] %d\n", l);
  return l;
}

/**
 * 指定したページの選択範囲の文字数をカウントする
 * @param {Number} pageNum 
 * @returns 文字数
 * @note 改行コードを含む
 */
function countCharactersSelected (pageNum) {
  let l = 0;
  let ldbg = 0;  // DEBUG
  const txtarea = getTextarea(pageNum);
  if(txtarea != null){
    const {selectionStart, selectionEnd} = txtarea;
    l = txtarea.value.substring(selectionStart, selectionEnd).length;
    ldbg = Math.abs(selectionEnd - selectionStart);  // これでもOK?
  } else {
    // textarea取得失敗
    l = -1;
  }
  console.log("[countCharactersSelected] %d, %d\n", l, ldbg);
  return l;
}

function handleKeyPress(event){
    if(keyParser(event, "s", ctrlKey=true)){
        saveFile();
    }
    if(keyParser(event, ".", ctrlKey=true, shiftKey=true)){
        // フォントサイズ大
        setFontSize(fontSize+1);
        console.log("fontsize big");
    }
    if(keyParser(event, ",", ctrlKey=true, shiftKey=true)){
        // フォントサイズ小
        setFontSize(fontSize-1);
        console.log("fontsize small");
    }
    if(keyParser(event,"Tab", ctrlKey=true, shiftKey=false)){
        // 面移動
        nextPage();
        console.log("next page");
    }
    if(keyParser(event, "Tab", ctrlKey=true, shiftKey=true)){
        // 面移動（逆）
        prevPage();
        console.log("prev page")
    }
}

/* いろいろお試し */
// window.addEventListener("keyup", handleKeyPress, true);
// window.addEventListener("change", (e) => {console.log("hoge")});
// window.addEventListener("input", (e) => {console.log("input! data=(%s) isComposing=%s detail=%s", e.data, e.isComposing, e.detail)});

// ---------------------------------------------------
//     UI初期設定
// ---------------------------------------------------

/**
 * キーバインドをセットする
 */
function setKeyBind () {
  Mousetrap.bind('ctrl+s', saveFile);
  Mousetrap.bind('ctrl+tab', nextPage);
  Mousetrap.bind('ctrl+shift+tab', prevPage);
  Mousetrap.bind('ctrl+shift+.', () => {
    setFontSize(fontSize + 1);
  });
  Mousetrap.bind('ctrl+shift+,', () => {
    setFontSize(fontSize - 1);
  });
  Mousetrap.bind('ctrl+t', switchTextboxFocus);
  Mousetrap.bind('ctrl+l', () => {
    setLockStatus(nowPage);
    updateLockStatus();
  });
  Mousetrap.bind('ctrl+shift+]', () => {  // DEBUG
    countCharacters(nowPage);
    countCharactersSelected(nowPage);
  });
}

listen(TauriEvent.DRAG_DROP, (event) => {
  console.log("DRAG_DROP");
  console.log(event);
  const paths = event.payload.paths;
  /* ファイルのドラッグアンドドロップなら1以上となる（テキストなどは0） */
  if(paths.length > 0){
    const path = paths[0];  // 1つ目のファイルのみ有効とする
    const data = {
      page_num: nowPage,
      path: path,
    };
    console.log(data);
    invoke("cmd_load_file", {payload: data})
      .then((message) => {
        // 読込成功
        console.log('[DRAG_DROP] 読込成功');
        console.log(message);
        const txtarea = getTextarea(message.page_num);
        const filenameBox = getFilenameTextbox(message.page_num);
        console.log(filenameBox);

        txtarea.value = message.memo.text;
        filenameBox.value = message.memo.filename;

        // 念のため未保存フラグをfalseにする（メインプロセス側でclear_memoを呼び出しているためそれで消える）
        unsaveList[message.page_num] = false;
        updateUnsavedStatus(unsaveList[nowPage]); // 現在表示している面(nowPage)の未保存表示を更新
      })
      .catch((error) => {
        console.log("[DRAG_DROP] 読込失敗: error=%s", error);
        // 何もしない
    });
    // return true;
  }
})

/**
 * ロック状態を反映する
 * 面切り替え後に呼び出す
 */
function updateLockStatusUI () {
  console.log("updateLockStatusUI");
  if (lockStatus.length === MAX_PAGENUM) {
    const saveButton = document.getElementById('save_button');
    saveButton.disabled = lockStatus[nowPage];
    for (let i = 0; i < lockStatus.length; i++) {
      const filenameBox = getFilenameTextbox(i);
      const txtarea = getTextarea(i);
      filenameBox.readOnly = lockStatus[i];
      txtarea.readOnly = lockStatus[i];
    }
  }
}

/**
 * タブ入力を有効化するコールバック
 */
function onTabKey (e) {
  /* tabキーのkeyCodeは9, 面移動でtabキーを使うため、ctrl同時押しは抑止する. （念のためaltも） */
  if ((e.keyCode === 9) &&
        (e.altKey === false) &&
        (e.ctrlKey === false)) {
    /* デフォルト動作を停止 */
    e.preventDefault();
    const obj = e.target;

    /* カーソル位置、カーソルの左右の文字列を取得 */
    const cursorPosition = obj.selectionStart;
    const cursorLeft = obj.value.substr(0, cursorPosition);
    const cursorRight = obj.value.substr(cursorPosition, obj.value.length);

    /* タブ文字を挟む */
    obj.value = cursorLeft + '\t' + cursorRight;

    /* カーソル位置をタブ文字の後ろに移動 */
    obj.selectionEnd = cursorPosition + 1;
  }
}

/**
 * タブ入力を有効化する
 */
function enableTabkey () {
  for (let i = 0; i < MAX_PAGENUM; i++) {
    const txtarea = getTextarea(i);
    txtarea.addEventListener('keydown', onTabKey);
  }
}

/**
 * 変更通知を行うコールバック（未保存検知用）
 */
function notifyChangeCB () {
  invoke("cmd_file_unsaved", {pageNum: nowPage});
  unsaveList[nowPage] = true;        // 未保存フラグを立てる
  updateUnsavedStatus(unsaveList[nowPage]); // 現在表示している面(nowPage)の未保存表示を更新
}

/**
 * 変更通知を有効化する(未保存検知用)
 */
function enableDetectChange () {
  for (let i = 0; i < MAX_PAGENUM; i++) {
    const txtarea = getTextarea(i);
    txtarea.addEventListener('input', notifyChangeCB);
  }
}

/**
 * UI関連設定を行う
 * @param {Number} fontsize  フォントサイズ
 * @param {String} font      フォント
 */
function setUiSetting(fontsize, font){
  for (let i = 0; i < MAX_PAGENUM; i++) {
    const txtarea = getTextarea(i);
    setFontSize(fontsize);
    txtarea.style.fontFamily = font;
  }
}

/**
 * イベントリスナーを登録する
 */
async function registerEventListener () {
  /**
   * ロック状態を受け取り、各面に反映する
   */
  await listen("set-lock-status", (event) => {
    console.log(event.payload);
    lockStatus = event.payload;
    updateLockStatusUI();
  })

  /**
   * メモクリア情報を受け取り、面に反映する
   */
  await listen("clear-memo", (event) => {
    console.log("clear-memo");
    console.log(event.payload);
    const pagenum = event.payload;
    const txtarea = getTextarea(pagenum);
    const filenameTextbox = getFilenameTextbox(pagenum);
    txtarea.value = '';
    filenameTextbox.value = '';
    unsaveList[pagenum] = false;
    updateUnsavedStatus(unsaveList[nowPage]); // 現在表示している面(nowPage)の未保存表示を更新
  })

  /**
   * 設定をセットする
   * ・フォントサイズ
   * ・フォント
   */
  await listen("set-settings", (event) => {
    console.log("set-settings");
    console.log(event.payload);
    setUiSetting(event.payload.fontsize, event.payload.font);
    // setUiSettingにて実行するためコメントアウト
    // for (let i = 0; i < MAX_PAGENUM; i++) {
    //   const txtarea = getTextarea(i);
    //   txtarea.style.fontSize = setFontSize(event.payload.fontsize);
    //   txtarea.style.fontFamily = event.payload.font;
    // }
  })

  /**
   * メモの内容を更新する
   * 
   * Note: ReloadEncodingで使用する想定
   */
  await listen("load-memo", (event) => {
    // TODO: クローン処理なのでまとめたい
    console.log("load-memo");
    console.log(event.payload); // LoadedMemoPayload

      const data = event.payload;
      const txtarea = getTextarea(data.page_num);
      const filenameBox = getFilenameTextbox(data.page_num);
      txtarea.value = data.memo.text;
      filenameBox.value = data.memo.filename;

      unsaveList[data.page_num] = false;
      updateUnsavedStatus(unsaveList[nowPage]); // 現在表示している面(nowPage)の未保存表示を更新
  });
}
