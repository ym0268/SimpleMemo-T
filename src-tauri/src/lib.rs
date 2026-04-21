use tauri::{AppHandle, Emitter, Manager};
use tauri::menu::{Submenu, MenuItem, MenuBuilder, PredefinedMenuItem};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use serde::{Serialize, Deserialize};
use serde_json;
use std::fs::{File, OpenOptions, metadata};
use std::io::{Read, Write, BufReader, ErrorKind};
use std::sync::Mutex;
use std::path::{Path, PathBuf};
use encoding_rs::{UTF_8, SHIFT_JIS};
use chardetng::{EncodingDetector, Utf8Detection, Iso2022JpDetection};
use sanitize_filename;
use log::{error, warn, info, debug, trace};
use simplelog;

const FILE_WARNING_SIZE: u64 = 1 * 1024 * 1024; // 1MB  TODO: 可変にしたい
const SETTING_FILENAME: &str = "settings.json";
const LOG_FILENAME: &str = "simplememo.log";
const MAX_PAGENUM: u32 = 3;                // ページ数  TODO: 可変にしたい

// WINDOW LABEL
const WINDOW_LABEL_MAIN: &str = "main";
const WINDOW_LABEL_GLOBAL_SETTING: &str = "global_setting_window";
const WINDOW_LABEL_LOCAL_SETTING: &str = "local_setting_window";
const WINDOW_LABEL_RELOAD_ENCODING: &str = "reload_encoding_window";
const WINDOW_LABEL_VERSION: &str = "version_window";

// MENU ID
const CONTEXT_MENU_ID_ALWAYS_TOP: &str = "always-top";
const CONTEXT_MENU_ID_LOCK: &str = "lock";
const CONTEXT_MENU_ID_CLEAR_SUBMENU: &str = "clear-submenu";
const CONTEXT_MENU_ID_CLEAR: &str = "clear";
const CONTEXT_MENU_ID_GLOBAL_SETTING: &str = "global-setting";
const CONTEXT_MENU_ID_LOCAL_SETTING: &str = "local-setting";
const CONTEXT_MENU_ID_ENCODING: &str = "encoding";
const CONTEXT_MENU_ID_VERSION: &str = "version";

// DIALOG TITLE
const DIALOG_TITLE: &str = "SimpleMemo";


const USE_DEV_TOOL: bool = false;                // デバッグ用

#[derive(PartialEq, Debug, Clone, Copy, Serialize, Deserialize)]
enum MemoError {
    Ok,                 // エラーなし
    FileExist,          // ファイルが存在する
    NoEntry,            // ファイルやディレクトリが存在しない（不正なファイル名を含む）
    NoDirectory,        // ディレクトリが存在しない
    LongPath,           // パスが長すぎる
    InvalidFileName,    // ファイル名が不正
    NoFileName,         // ファイル名が入力されていない
    SameName,           // 開いているファイルと同名
    Permission,         // 権限がない
    LargeFile,          // ファイルサイズが巨大
    AlreadyOpen,        // すでにオープン済み
    Decode,             // デコードエラー
    LeaveMemo,          // メモが残っている
    Busy,               // リソースが使用中かロックされている
    InvalidFontSize,    // フォントサイズの値が不正
    Param,              // パラメータが不正
    Error,              // その他エラー（汎用）
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
enum EncType {
    ShiftJis,
    Utf8,
}

struct AppData{
    memo_manager: MemoManager,
    allow_main_close: bool,
    setting_filepath: String,
}

/// 設定・ログ保存先のディレクトリを取得する
/// - portable feature有効: 実行exeと同じディレクトリ
/// - それ以外: app_config_dir
fn resolve_app_data_dir(app: &AppHandle) -> Result<PathBuf, std::io::Error> {
    if cfg!(feature = "portable") {
        let exe_path = std::env::current_exe()?;
        match exe_path.parent() {
            Some(dir) => Ok(dir.to_path_buf()),
            None => Err(std::io::Error::new(
                ErrorKind::Other,
                "failed to resolve executable directory",
            )),
        }
    } else {
        app.path().app_config_dir().map_err(|err| {
            std::io::Error::new(
                ErrorKind::Other,
                format!("failed to resolve app config dir: {:?}", err),
            )
        })
    }
}

/**
 * 文字コード変換クラス
 */
struct EncodingConverter;
impl EncodingConverter {
    // BOM (未使用のためコメントアウト)
    // const BOM_UTF8: [u8; 3] = [0xEF, 0xBB, 0xBF];
    // const BOM_UTF16BE: [u8; 2] = [0xFE, 0xFF];
    // const BOM_UTF16LE: [u8; 2] = [0xFF, 0xFE];

    /// BOMの種類を判別する
    /// note: BOMに対応させてないため未使用
    // pub fn check_bom(bytes: &[u8]) -> Option<&Encoding> {
    //     if (bytes.len() >= 3) && (bytes[0] == Self::BOM_UTF8[0]) && (bytes[1] == Self::BOM_UTF8[1]) && (bytes[2] == Self::BOM_UTF8[2]) {
    //         Some(UTF_8)
    //     } else if (bytes.len() >= 2) && (bytes[0] == Self::BOM_UTF16BE[0]) && (bytes[1] == Self::BOM_UTF16BE[1]) {
    //         Some(UTF_16BE)
    //     } else if (bytes.len() >= 2) && (bytes[0] == Self::BOM_UTF16LE[0]) && (bytes[1] == Self::BOM_UTF16LE[1]) {
    //         Some(UTF_16LE)
    //     } else {
    //         None
    //     }
    // }

    /// 文字コードを推定する  
    /// SJISまたはUTF-8のみ対応。 推定できない場合はNoneを返す。
    pub fn detect(bytes: &[u8]) -> Option<EncType> {
        trace!("[EncodingConverter::detect]");
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
        detector.feed(bytes, true);

        let encoding = detector.guess(None, Utf8Detection::Allow);

        if UTF_8.eq(encoding) {
            debug!("[EncodingConverter::detect] guess: UTF-8.");
            Some(EncType::Utf8)
        } else if SHIFT_JIS.eq(encoding) {
            debug!("[EncodingConverter::detect] guess: ShiftJIS.");
            Some(EncType::ShiftJis)
        } else {
            warn!("[EncodingConverter::detect] Unsupported encoding. ({:?})", encoding);
            None    // 他が検出されても未対応ということにする
        }
    }

     /// バイト列を指定した文字コードで文字列に変換する  
     /// ignore_errorがtrueの場合、デコードエラーがあっても可能な限り文字列に変換する。
    pub fn decode(bytes: &[u8], enc: EncType, ignore_error: bool) -> Result<String, String> {
        trace!("[EncodingConverter::decode]");
        let encoder = match enc {
            EncType::Utf8 => UTF_8,
            EncType::ShiftJis => SHIFT_JIS,
        };
        let (result, _, err) = encoder.decode(bytes);
        if err {
            debug!("[EncodingConverter::decode] Decode error. (enc: {:?})", enc);
            if !ignore_error {
                return Err("Decode error.".to_string());
            }
        }
        Ok(result.into())
    }

    /// 文字列を指定した文字コードでバイト列に変換する
    pub fn convert(text: &str, enc: EncType) -> Result<Vec<u8>, String> {
        /* 
        * Cowに関して
        * https://blog.ojisan.io/many-copies-original-sin/
        */
        trace!("[EncodingConverter::convert]");
        let encoder = match enc {
            EncType::Utf8 => UTF_8,
            EncType::ShiftJis => SHIFT_JIS,
        };
        let (result, _, err )= encoder.encode(text);

        if err {
            error!("[EncodingConverter::convert] Encoding error. (enc: {:?})", enc);
            return Err("Encoding error.".to_string());
        }

        Ok(result.into())
    }
}

/// メモクラス
#[derive(Clone, Debug)]
struct Memo {
    // グローバル設定
    default_save_path: String,      // デフォルトの保存先パス
    default_encoding: EncType,      // デフォルトの文字コード
    auto_encoding: bool,            // 自動文字コード識別を行うか

    // 個別設定
    save_dir: String,               // 保存先パス
    encoding: EncType,              // 文字コード
    is_external_file: bool,         // 外部ファイルか
    filename: String,               // ファイル名（初回保存時にセットする） <- 不要かも
    save_count: u32,                // 保存回数 <- 回数ではなく初回保存したかどうかで良くない？
    fullpath: String,               // 保存先フルパス（保存先ファイル存在判定用、保存に成功したパス）
    unsaved: bool,                  // 未保存か
    locked: bool,                   // ロック（編集不可）状態か
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LoadedMemo {
    text: String,
    filename: String,
}

impl Memo {
    /// コンストラクタ
    pub fn new(default_save_path: String, default_encoding: EncType, auto_encoding: bool) -> Self {
        trace!("[Memo::new]");
        debug!("[Memo::new] default_save_path: {}, default_encoding: {:?}, auto_encoding: {}", default_save_path, default_encoding, auto_encoding);
        Memo {
            default_save_path: default_save_path.clone(),
            default_encoding: default_encoding.clone(),
            auto_encoding: auto_encoding,
            save_dir: default_save_path.clone(),
            encoding: default_encoding.clone(),
            is_external_file: false,
            filename: "".to_string(),
            save_count: 0,
            fullpath: "".to_string(),     // TODO: 0文字にするか、Option型にするかは検討する
            unsaved: false,
            locked: false,
        }
    }

    /// 未保存フラグを立てる
    pub fn set_unsaved(&mut self) {
        trace!("[Memo::set_unsaved]");
        self.unsaved = true;
    }

    /// 外部ファイルを読み込む  
    /// * `fullpath` ファイルのフルパス  
    /// * `ignore_fsize` ファイルサイズ警告を無視するか
    /// * `overwrite` 上書きするか
    /// * `encoding` 文字コード
    /// 
    /// 読み込むファイルのサイズが一定以上の場合、警告を出す。  
    /// メモが残っている場合、警告を出す。
    pub fn load(&mut self, fullpath: String, ignore_fsize: bool, overwrite: bool, encoding: Option<EncType>) -> Result<LoadedMemo, MemoError> {
        trace!("[Memo::load]");
        debug!("[Memo::load] fullpath: {}, ignore_fsize: {}, overwrite: {}, encoding: {:?}", fullpath, ignore_fsize, overwrite, encoding);

        let mut err = Ok(());
        let mut buf: Vec<u8> = Vec::new();
        let mut text: String = "".to_string();

        if overwrite == false && (self.unsaved || self.fullpath != "") {
            // メモが残っているか確認（未保存でも記入済みの場合、保存先パスがセットされている場合（一度保存したか、外部読み込みしたか））
            debug!("[Memo::load] Leave memo. unsaved: {}, fullpath: {}", self.unsaved, self.fullpath);
            err = Err(MemoError::LeaveMemo);
        } else if !Path::is_file(Path::new(&fullpath)) {
            /* ファイルの存在確認 */
            /* TODO: ファイルのアクセス権限判定もすべき. std::path::PathBuf, std::fs::Metadata */
            debug!("[Memo::load] file not found. fullpath: {}", fullpath);
            err = Err(MemoError::NoEntry);
        } else {
            /* ファイルサイズ判定 */
            let stat = metadata(fullpath.clone());
            if stat.is_ok() {
                let fsize = stat.unwrap().len();
                if ignore_fsize == false && fsize > FILE_WARNING_SIZE {
                    debug!("[Memo::load] file size warning. fsize: {}, limit: {}", fsize, FILE_WARNING_SIZE);
                    err = Err(MemoError::LargeFile);
                }
            } else {
                /* metadata取得失敗 */
                warn!("[Memo::load] metadata error. fullpath: {}", fullpath);
                err = Err(MemoError::Error);
            }
        }
        if err.is_ok() {
            // ファイル読み込み
            let f = File::open(fullpath.clone());
            if f.is_ok() {
                match f.unwrap().read_to_end(&mut buf) {
                    Ok(_) => {},
                    Err(_) => {
                        error!("[Memo::load] File read_to_end error.");
                        err = Err(MemoError::Error);
                    },
                }
            } else {
                match f.unwrap_err().kind() {
                    ErrorKind::NotFound => {
                        warn!("[Memo::load] file not found. fullpath: {}", fullpath);
                        err = Err(MemoError::NoEntry);
                    },
                    ErrorKind::PermissionDenied => {
                        warn!("Memo::load] Permission denied. fullpath: {}", fullpath);
                        err = Err(MemoError::Permission);   // BUSYもこれになるかも
                    },
                    _ => {
                        error!("[Memo::load] File open error. fullpath: {}", fullpath);
                        err = Err(MemoError::Error);
                    },
                }
            }
        }

        let mut from: EncType = self.default_encoding;
        if err.is_ok() {
            // 文字コード変換
            if encoding.is_some() {
                debug!("[Memo::load] Use specified encoding: {:?}", encoding.unwrap());
                from = encoding.unwrap();
            } else if self.auto_encoding {
                from = match EncodingConverter::detect(&buf) {
                    Some(value) => value,
                    None => {
                        warn!("[Memo::load] Auto detect failed. Use default encoding. {:?}", self.default_encoding);
                        self.default_encoding   // 推定できなければデフォルトエンコードを使用する
                    }
                };
            } else {
                debug!("[Memo::load] encoding is not specified. Use {:?}.", self.encoding);
                from = self.encoding;
            }
            // デコードエラーは無視する(ignore_error = trueで必ずOkしか返らない) TODO: デコードエラーを無視するかどうかは検討する
            text = match EncodingConverter::decode(&buf, from, true) {
                Ok(value) => value.clone(),
                Err(_) => {
                    error!("[Memo::load] Decode error.");
                    err = Err(MemoError::Decode);
                    "".to_string()
                }
            };
        }

        if err.is_ok(){
            // 読込成功
            self.clear();
            self.set_external_file(fullpath);
            self.set_encoding(from); // 変換に使用した文字コードを保持する
            debug!("[Memo::load] Load success. filename: {}", self.filename);
            Ok(LoadedMemo{
                text: text,
                filename: self.filename.clone(),
            })
        } else {
            // ここでself.clear()は呼び出さないこと。LeaveMemoの時もclearが走ってしまうため。
            debug!("[Memo::load] Load failed. err: {:?}", err.unwrap_err());
            Err(err.unwrap_err())
        }
    }

    /// ファイル名に不正な文字が含まれていないか確認する  
    fn check_filename(filename: &String) -> bool {
        trace!("[Memo::check_filename]");
        debug!("[Memo::check_filename] input filename: {}", filename);
        let res = sanitize_filename::sanitize(filename);
        debug!("[Memo::check_filename] sanitized filename: {}", filename);
        // filenameとresの文字列比較を行う
        if filename.eq(&res) {
            false
        } else {
            info!("[Memo::check_filename] contains invalid characters. old: {}, new: {}", filename, res);
            true
        }
    }

    /// ファイル名に拡張子を付加する  
    /// 内部生成ファイルの時に.txtを付加するため。  
    fn add_extension(&self, filename: &String) -> String {
        trace!("[Memo::add_extension]");
        if !self.is_external_file {
            debug!("[Memo::add_extension] add .txt extension. input filename: {}", filename);
            filename.clone() + ".txt"
        } else {
            debug!("[Memo::add_extension] no extension. input filename: {}", filename);
            filename.clone()
        }
    }

    /// ファイルを保存する  
    /// * `filename` ファイル名（フォルダ名含まず）  
    /// * `text` 保存するテキスト
    /// * `overwrite` ファイルが存在する場合に上書きするか（初回書き込みのみ有効）
    /// 
    /// ### Notes
    /// アプリ内新規作成ファイルの場合、ファイル名に拡張子をつけない（.txtを自動付加するため）。  
    /// ファイル名がオブジェクト内に保持しているファイル名と一致しない場合、新規ファイルとして保存する。
    pub fn save(&mut self, filename: String, text: &String, overwrite: bool) -> Result<(), MemoError> {
        trace!("[Memo::save]");
        debug!("[Memo::save] filename: {}, text: {}, overwrite: {}", filename, text, overwrite);
        let mut err = MemoError::Ok;
        let mut _overwrite = false;       // デフォルトは上書き禁止モード

        if filename == "" {
            // ファイル名が入力されていない
            warn!("[Memo::save] No filename.");
            err = MemoError::NoFileName;
        } else if !Path::is_dir(Path::new(&self.save_dir)) {
            // 保存先が存在しない
            warn!("[Memo::save] No directory. save_dir: {}", self.save_dir);
            err = MemoError::NoDirectory;
        } else if Self::check_filename(&filename) {
            // ファイル名が不正
            warn!("[Memo::save] Invalid filename. filename: {}", filename);
            err = MemoError::InvalidFileName;
        } else {
            // - 新規作成ファイルの初回保存
            // - 新規作成ファイルの2回目以降の保存
            // - 新規作成ファイルの2回目以降の保存で、ファイル名を変更した場合
            // - 外部読込ファイルの上書き保存
            // - 外部読込ファイルを保存せずに、ファイル名を変更して保存した場合
            // - 外部読込ファイルを保存した後、ファイル名を変更して保存した場合
            // 上記それぞれで既存ファイル名と重複する場合を考慮する
            // 外部読込ファイルの場合、拡張子の付与有無が変更になった場合も考慮する（拡張子付与したときに既存ファイルと重複した場合を考慮すること）

            let tmp_is_external_file: bool = self.is_external_file;   // 保存失敗時に元のis_external_fileに戻すための変数
            let tmp_encoding: EncType = self.encoding;   // 保存失敗時に元のencodingに戻すための変数

            let mut filename_with_ext = self.add_extension(&filename);
            let mut savepath = Path::new(&self.save_dir).join(filename_with_ext);

            if (self.is_external_file == true) && (savepath != Path::new(&self.fullpath)) {
                // 外部ファイルを上書きせずに別名で保存しようとしている場合は、外部ファイルではなく新規ファイルとして保存する
                debug!("[Memo::save] Save as new file. Not overwrite external file. savepath: {:?}, fullpath: {:?}", savepath, self.fullpath);
                self.is_external_file = false;
                filename_with_ext = self.add_extension(&filename);
                savepath = Path::new(&self.default_save_path).join(filename_with_ext);

            }

            // 外部読み込みファイル、上書き可または保存先が前回と一致した場合は上書きモード
            if (self.is_external_file == true) || (overwrite == true) || (savepath == Path::new(&self.fullpath)) {
                debug!("[Memo::save] Overwrite mode.");
                _overwrite = true;
            }

            // 外部ファイルではなく、新規保存だと思われる場合はデフォルトエンコーディングをセット
            // もともと外部読込ファイルだった場合は変更しない（外部読込後にファイル名を変更した場合）
            if (tmp_is_external_file == false) && savepath != Path::new(&self.fullpath) {
                debug!("[Memo::save] New file. Set default encoding. encoding: {:?}", self.default_encoding);
                self.encoding = self.default_encoding.clone();
            }

            // 文字コード変換
            let buf = EncodingConverter::convert(text, self.encoding);
            if buf.is_err() {
                error!("[Memo::save] Encoding convert error. encoding: {:?}", self.encoding);
                // エラーのため、エンコードと外部ファイルフラグを保存前の状態に戻す
                self.encoding = tmp_encoding;
                self.is_external_file = tmp_is_external_file;
                err = MemoError::Error;
            }

            // 保存
            if err == MemoError::Ok {
                let f = match _overwrite {
                    false => OpenOptions::new().write(true).create_new(true).open(Path::new(&savepath)),
                    true => OpenOptions::new().write(true).create(true).truncate(true).open(Path::new(&savepath)),
                };
                if f.is_ok() {
                    debug!("[Memo::save] File open success.");
                    match f.unwrap().write_all(&buf.unwrap()) {
                        Ok(_) => {
                            if savepath != Path::new(&self.fullpath) {
                                // 新規保存
                                debug!("[Memo::save] New file save. filename: {}", filename);
                                self.set_new_file();
                            }
                            self.fullpath = savepath.to_str().unwrap().into();  // 保存先を保存
                            self.save_count += 1;                               // 保存回数を更新
                            self.unsaved = false;                               // 保存済みに変更
                            debug!("[Memo::save] File write success. fullpath: {:?}, save_count: {}", self.fullpath, self.save_count);
                        },
                        Err(_) => {
                            error!("[Memo::save] File write error.");
                            // エラーのため、エンコードと外部ファイルフラグを保存前の状態に戻す
                            self.encoding = tmp_encoding;
                            self.is_external_file = tmp_is_external_file;
                            err = MemoError::Error;
                        }
                    }

                } else {
                    // エラーのため、エンコードと外部ファイルフラグを保存前の状態に戻す
                    self.encoding = tmp_encoding;
                    self.is_external_file = tmp_is_external_file;
                    let errkind = f.unwrap_err().kind();
                    error!("[Memo::save] File open error. set last encoding. encoding: {:?}, errkind: {:?}", self.encoding, errkind);

                    match errkind {
                        ErrorKind::NotFound => err = MemoError::NoEntry,
                        ErrorKind::AlreadyExists => err = MemoError::FileExist,
                        ErrorKind::PermissionDenied => err = MemoError::Permission, // BUSY含む
                        _ => err = MemoError::Error,
                    }
                }
            }
        }

        /* 
         * saveCount: メモデータから取得可能
         * isExternalFile: 同上
         */
        match err {
            MemoError::Ok => Ok(()),
            _ => Err(err),
        }
    }

    /// メモの内容をクリアする
    /// UI側クリア時にコールする。
    fn clear(&mut self) {
        trace!("[Memo::clear]");
        debug!("[Memo::clear] Clear memo.");
        self.save_dir = self.default_save_path.clone();
        self.encoding = self.default_encoding;
        self.is_external_file = false;
        self.save_count = 0;
        self.fullpath = "".to_string();
        self.unsaved = false;
    }

    /// 外部ファイル情報をセットする  
    /// 外部ファイルを読み込んだ場合に実行すること。
    fn set_external_file(&mut self, fullpath: String) {
        trace!("[Memo::set_external_file]");
        debug!("[Memo::set_external_file] fullpath: {}", fullpath);
        let p = Path::new(&fullpath);
        self.is_external_file = true;
        self.save_dir = p.parent().unwrap().to_string_lossy().into();
        self.filename = p.file_name().unwrap().to_string_lossy().into();
        self.fullpath = fullpath;
    }

    /// 新規ファイル情報をセットする  
    /// 新規ファイルを作成するときに実行すること。  
    fn set_new_file(&mut self) {
        trace!("[Memo::set_new_file]");
        debug!("[Memo::set_new_file] Set new file.");
        self.save_dir = self.default_save_path.clone();
        self.is_external_file = false;
        self.save_count = 0;
    }

    /// デフォルトの保存先パスをセットする
    pub fn set_default_savepath(&mut self, dir: String) {
        trace!("[Memo::set_default_savepath]");
        debug!("[Memo::set_default_savepath] dir: {}", dir);
        self.default_save_path = dir.clone();
        // 外部読込ファイルでなければ保存先を変更する（一度保存済みのファイルも更新されるのは仕様）
        if !self.is_external_file {
            debug!("[Memo::set_default_savepath] Internal file.");
            self.save_dir = dir.clone();
        }
    }

    /// デフォルトの文字コードをセットする
    pub fn set_default_encoding(&mut self, enc: EncType) {
        trace!("[Memo::set_default_encoding]");
        debug!("[Memo::set_default_encoding] enc: {:?}", enc);
        self.default_encoding = enc;
        // 外部ファイルではなく、1度も保存していなければデフォルトエンコードを設定
        if (self.is_external_file == false) && (self.save_count == 0) {
            debug!("[Memo::set_default_encoding] Internal file and not saved yet. Set default encoding.");
            self.encoding = self.default_encoding.clone();
        }
    }

    /// 文字コードをセットする
    pub fn set_encoding(&mut self, enc: EncType) {
        trace!("[Memo::set_encoding]");
        debug!("[Memo::set_encoding] enc: {:?}", enc);
        self.encoding = enc;
    }

    /// 文字コード自動判別をセットする
    pub fn set_auto_encoding(&mut self, auto_encoding: bool) {
        trace!("[Memo::set_auto_encoding]");
        debug!("[Memo::set_auto_encoding] auto_encoding: {}", auto_encoding);
        self.auto_encoding = auto_encoding;
    }

}


/// メモの設定クラス
#[derive(Serialize, Deserialize, Clone, Debug)]
struct MemoSetting {
    version: u32,
    savepath: String,
    fontsize: u32,
    font: String,
    top_most: bool,
    encoding: EncType,
    auto_encoding: bool,
    file_size_warning_th: u64,
    load_last_file: bool,
    no_close_dialog: bool,
    auto_save: bool,
    auto_save_span: u32,
    auto_lock: bool,
}
impl MemoSetting {
    const VERSION: u32 = 1;

    /// コンストラクタ
    pub fn new() -> Self {
        trace!("[MemoSetting::new]");
        Self {
            version: Self::VERSION,
            savepath: "./".to_string(),
            fontsize: 16,
            font: "Yu Gothic UI".to_string(),
            top_most: true,
            encoding: EncType::Utf8,
            auto_encoding: true,
            file_size_warning_th: 1 * 1024 * 1024,
            load_last_file: false,
            no_close_dialog: false,
            auto_save: false,
            auto_save_span: 5,
            auto_lock: false,
        }
    }

    /// 設定を読み込む
    pub fn load(&mut self, filepath: String) -> Result<(), MemoError>{
        trace!("[MemoSetting::load]");
        debug!("[MemoSetting::load] filepath: {}", filepath);
        let f = File::open(filepath);
        if f.is_ok() {
            debug!("[MemoSetting::load] File open success.");
            let reader: BufReader<File> = BufReader::new(f.unwrap());
            let setting = serde_json::from_reader::<BufReader<File>, Self>(reader);
            if setting.is_ok() {
                debug!("[MemoSetting::load] Deserialize success.");
                *self = setting.unwrap();
                Ok(())
            } else {
                error!("[MemoSetting::load] Deserialize error.");
                Err(MemoError::Param)
            }
        } else {
            let errkind = f.as_ref().unwrap_err().kind();
            error!("[MemoSetting::load] File open error. errkind: {:?}", errkind);
            match errkind {
                ErrorKind::NotFound => Err(MemoError::NoEntry),
                ErrorKind::PermissionDenied => Err(MemoError::Permission),
                _ => Err(MemoError::Error),
            }
        }
    }

    /// 設定を保存する
    pub fn save(&self, filepath: String)-> Result<(), MemoError> {
        trace!("[MemoSetting::save]");
        debug!("[MemoSetting::save] filepath: {}", filepath);
        let serialized = match serde_json::to_string(self){
            Ok(value) => value,
            Err(_) => return Err(MemoError::Error),
        };
        let mut f = match OpenOptions::new().write(true).create(true).truncate(true).open(&filepath) {
            Ok(value) => value,
            Err(error) => {
                match error.kind() {
                    ErrorKind::PermissionDenied => return Err(MemoError::Permission),
                    _ => return Err(MemoError::Error),
                }
            }
        };
        match f.write_all(serialized.as_bytes()) {
            Ok(_) => Ok(()),
            Err(_) => Err(MemoError::Error),
        }
    }

    /// 設定を更新する
    /// 未使用のためコメントアウト
    // pub fn update(&mut self, data: String) -> Result<(), MemoError> {
    //     let setting : Self = match serde_json::from_str(&data) {
    //         Ok(value) => value,
    //         Err(_) => return Err(MemoError::Error),
    //     };
    //     *self = setting;
    //     Ok(())
    // }

    /// 設定値が正しいか検証する
    pub fn validate(&self) -> Result<(), MemoError> {
        /* フォントサイズ確認エラー  */
        if self.fontsize < 1 || self.fontsize > 100 {
            warn!("[MemoSetting::validate] Invalid font size. fontsize: {}", self.fontsize);
            return Err(MemoError::InvalidFontSize);
        }
        Ok(())
    }
}

/// メモ管理クラス
struct MemoManager {
    memo_num: u32,
    memo_setting: MemoSetting,
    memo_list: Vec<Memo>,
    page_num: usize,
}

impl MemoManager {
    /// コンストラクタ
    pub fn new(memo_num: u32, memo_setting: MemoSetting) -> Self {
        trace!("[MemoManager::new]");
        debug!("[MemoManager::new] memo_num: {}, memo_setting: {:?}", memo_num, memo_setting);
        Self {
            memo_num: memo_num,
            memo_setting: memo_setting.clone(),
            memo_list: vec![Memo::new(memo_setting.savepath, memo_setting.encoding, memo_setting.auto_encoding); memo_num as usize],
            page_num: 0,
        }
    }

    /// 指定したページ番号にメモを保存する
    pub fn save(&mut self, idx: usize, filename: String, text: &String, overwrite: bool) -> Result<(), MemoError> {
        trace!("[MemoManager::save]");
        debug!("[MemoManager::save] idx: {}, filename: {}, text: {}, overwrite: {}", idx, filename, text, overwrite);
        self.memo_list[idx].save(filename, text, overwrite)
        // ページ番号は本関数を呼び出すときに指定しているため、返す必要はないと判断し省略。
    }

    /// 指定したページ番号にメモを読み込む
    pub fn load(&mut self, idx: usize, fullpath: String, ignore_fsize: bool, overwrite: bool, encoding: Option<EncType>) -> Result<LoadedMemo, MemoError> {
        trace!("[MemoManager::load]");
        debug!("[MemoManager::load] idx: {}, fullpath: {}, ignore_fsize: {}, overwrite: {}, encoding: {:?}", idx, fullpath, ignore_fsize, overwrite, encoding);
        // 他の面で開いていないか確認する
        for i in 0..self.memo_num {
            if i != idx as u32 {
                if self.memo_list[i as usize].fullpath == fullpath {
                    // すでにオープン済み
                    warn!("MemoManager::load] Already open. idx: {}, fullpath: {}", idx, fullpath);
                    return Err(MemoError::AlreadyOpen);
                }
            }
        }

        self.memo_list[idx].load(fullpath, ignore_fsize, overwrite, encoding)
    }

    /// 未保存フラグを立てる
    pub fn set_unsaved(&mut self, idx: usize) {
        trace!("[MemoManager::set_unsaved]");
        debug!("[MemoManager::set_unsaved] idx: {}", idx);
        self.memo_list[idx].set_unsaved();
    }

    /// 未保存の面番号リストを取得する
    pub fn get_unsaved_list(&self) -> Vec<usize> {
        trace!("[MemoManager::get_unsaved_list]");
        let mut unsaved_list: Vec<usize> = Vec::new();

        for i in 0..self.memo_num {
            if self.memo_list[i as usize].unsaved == true {
                unsaved_list.push(i as usize);
            }
        }
        unsaved_list
    }

    /// ぺージ番号をセットする
    pub fn set_page_num(&mut self, idx: usize) {
        trace!("[MemoManager::set_page_num]");
        debug!("[MemoManager::set_page_num] idx: {}", idx);
        self.page_num = idx;
    }

    /// フォントサイズをセットする
    pub fn set_font_size(&mut self, fontsize: u32) {
        trace!("[MemoManager::set_font_size]");
        debug!("[MemoManager::set_font_size] fontsize: {}", fontsize);
        self.memo_setting.fontsize = fontsize;
    }

    /// ロック状態を切り替える
    pub fn toggle_lock_status(&mut self, idx: usize) -> bool {
        trace!("[MemoManager::toggle_lock_status]");
        debug!("[MemoManager::toggle_lock_status] idx: {}", idx);
        self.memo_list[idx].locked = !self.memo_list[idx].locked;
        self.memo_list[idx].locked
    }

    /// メモをクリアする
    pub fn clear_memo(&mut self, idx: usize) {
        trace!("[MemoManager::clear_memo]");
        debug!("[MemoManager::clear_memo] idx: {}", idx);
        // TODO: インデックス不正チェックを行う
        self.memo_list[idx].clear();
    }

    /// 各面のロック状態を取得する
    pub fn get_lock_status(&self) -> Vec<bool> {
        trace!("[MemoManager::get_lock_status]");
        let mut lock_list: Vec<bool> = Vec::new();
        for i in 0..self.memo_num {
            lock_list.push(self.memo_list[i as usize].locked);
        }
        lock_list
    }

    /// 現在の全体設定を取得する
    pub fn get_global_setting(&self) -> MemoSetting {
        trace!("[MemoManager::get_global_setting]");
        self.memo_setting.clone()
    }

    /// 全体設定をセットする
    pub fn set_global_setting(&mut self, setting: &MemoSetting) -> Result<(), MemoError> {
        trace!("[MemoManager::set_global_setting]");
        debug!("[MemoManager::set_global_setting] setting: {:?}", setting);
        let ret = setting.validate();
        if ret.is_ok(){
            self.memo_setting = setting.clone();

            // 各メモに反映
            for i in 0..self.memo_num {
                let idx = i as usize;
                self.memo_list[idx].set_default_savepath(self.memo_setting.savepath.clone());
                self.memo_list[idx].set_default_encoding(self.memo_setting.encoding);
                self.memo_list[idx].set_auto_encoding(self.memo_setting.auto_encoding);
            }
        } else {
            error!("[MemoManager::set_global_setting] error: {:?}", ret.unwrap_err());
        }
        ret
    }

    /// 設定をファイルから読み込む
    pub fn load_setting(&mut self, filepath: String) -> Result<(), MemoError> {
        trace!("[MemoManager::load_setting]");
        debug!("[MemoManager::load_setting] filepath: {}", filepath);
        match self.memo_setting.load(filepath) {
            Ok(_) => {
                self.set_global_setting(&self.memo_setting.clone())
            },
            Err(err) => {
                error!("[MemoManager::load_setting] error: {:?}", err);
                Err(MemoError::Error)
            }
        }
    }

    /// 設定をファイルに書き出す
    pub fn save_setting(&self, filepath: String) -> Result<(), MemoError> {
        trace!("[MemoManager::save_setting]");
        debug!("[MemoManager::save_setting] filepath: {}", filepath);
        self.memo_setting.save(filepath.clone())
    }

    /// 指定したページ番号のメモオブジェクトを取得する  
    /// Tauri版で新規追加  
    pub fn get_memo(&self, idx: usize) -> &Memo {
        trace!("[MemoManager::get_memo]");
        debug!("[MemoManager::get_memo] idx: {}", idx);
        &self.memo_list[idx]
    }

    /// 指定したページ番号のメモオブジェクトを取得する（可変参照）
    /// Tauri版で新規追加  
    pub fn get_memo_mut(&mut self, idx: usize) -> &mut Memo {
        trace!("[MemoManager::get_memo_mut]");
        debug!("[MemoManager::get_memo_mut] idx: {}", idx);
        &mut self.memo_list[idx]
    }
}

/// メモをクリアし、情報を送信する
fn clear_memo(app: &AppHandle, memo_manager: &mut MemoManager, idx: usize) -> Result<(), MemoError> {
    trace!("[clear_memo]");
    debug!("[clear_memo] idx: {}", idx);
    memo_manager.clear_memo(idx);
    match app.get_webview_window(WINDOW_LABEL_MAIN) {
        Some(x) => {
            match x.emit("clear-memo", idx) {
                Ok(_) => Ok(()),
                Err(_) => {
                    error!("[clear_memo] emit error.");
                    Err(MemoError::Error)
                }
            }
        },
        None => {
            error!("[clear_memo] main window not found.");
            Err(MemoError::Error)
        },
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UiSettingPayload {
    fontsize: u32,
    font: String,
    top_most: bool,
}

/// UIの設定を画面に適用する
fn set_ui_setting(app: &AppHandle, memo_setting: &MemoSetting) {
    trace!("[set_ui_setting]");
    debug!("[set_ui_setting] memo_setting: {:?}", memo_setting);
    match app.get_webview_window(WINDOW_LABEL_MAIN){
        Some(x) => {
            let payload = UiSettingPayload {
                fontsize: memo_setting.fontsize,
                font: memo_setting.font.clone(),
                top_most: memo_setting.top_most,
            };
            match x.set_always_on_top(memo_setting.top_most) {
                Ok(_) => {
                    debug!("[set_ui_setting] set_always_on_top: {:?}", memo_setting.top_most);
                },
                Err(_) => {
                    error!("[set_ui_setting] set_always_on_top error.");
                }
            }
            match x.emit("set-settings", payload) {
                Ok(_) => {},
                Err(_) => {
                    error!("[set_ui_setting] emit error.");
                }
            }

        },
        None => {
            error!("[set_ui_setting] main window not found.");
        },
    }
}

/// サブウィンドウが存在しているかを判定し、存在する場合はメインウィンドウを無効化、存在しない場合は有効化する
fn update_main_window_enabled(app: &AppHandle) {
    trace!("[update_main_window_enabled]");
    update_main_window_enabled_ignoring(app, None);
}

fn update_main_window_enabled_ignoring(app: &AppHandle, ignored_label: Option<&str>) {
    trace!("[update_main_window_enabled_ignoring]");
    debug!("[update_main_window_enabled_ignoring] ignored_label: {:?}", ignored_label);
    match app.get_webview_window(WINDOW_LABEL_MAIN) {
        Some(main_window) => {
            let has_sub_window = app
                .webview_windows()
                .keys()
                .any(|label| {
                    if label.as_str() == WINDOW_LABEL_MAIN {
                        return false;
                    }
                    match ignored_label {
                        Some(ignored) => label.as_str() != ignored,
                        None => true,
                    }
                });
            let enabled = !has_sub_window;
            if let Err(err) = main_window.set_enabled(enabled) {
                error!(
                    "[update_main_window_enabled] set_enabled({}) error: {:?}",
                    enabled, err
                );
            }
        },
        None => {
            error!("[update_main_window_enabled] main window not found.");
        },
    }
}


/* ===================================================== *
 * WINDOWS
 * ===================================================== */
/// 全体設定画面を作成する
fn create_global_setting_window(app: &tauri::AppHandle) {
    trace!("[create_global_setting_window]");
    let label = WINDOW_LABEL_GLOBAL_SETTING.to_string();
    let parent = app.get_webview_window(WINDOW_LABEL_MAIN.into()).unwrap();
    let parent_scale_factor = parent.scale_factor().unwrap();
    let parent_pos = parent
        .outer_position()
        .unwrap()
        .to_logical::<f64>(parent_scale_factor);
    let err = tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("main_setting.html".into()))
        .parent(&parent).unwrap()
        .title("全体設定 - SimpleMemo")
        .inner_size(550.0, 350.0)
        .position(parent_pos.x, parent_pos.y) // 親ウィンドウの位置
        .build();
    match err {
        Ok(_window) => {
            if USE_DEV_TOOL {
                #[cfg(debug_assertions)]
                _window.open_devtools();
            }
            update_main_window_enabled(app);
        },
        Err(tauri::Error::WebviewLabelAlreadyExists(_)) => {
            warn!("[create_global_setting_window] global_setting_window is already opened.");
            update_main_window_enabled(app);
        },
        Err(_) => {
            error!("[create_global_setting_window] Create window error.");
            panic!("Create window error");
        },
    }
}

/// 個別設定画面を作成する
fn create_local_setting_window(app: &tauri::AppHandle) {
    trace!("[create_local_setting_window]");
    let label = WINDOW_LABEL_LOCAL_SETTING.to_string();
    let parent = app.get_webview_window(WINDOW_LABEL_MAIN.into()).unwrap();
    let parent_scale_factor = parent.scale_factor().unwrap();
    let parent_pos = parent
        .outer_position()
        .unwrap()
        .to_logical::<f64>(parent_scale_factor);
    // parent.set_closable(false);  // Alt+F4が使えてしまうので意味なし
    // parent.set_content_protected(true).unwrap();
    let err = tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("local_setting.html".into()))
        .parent(&parent).unwrap()
        // .content_protected(true)
        .title("個別設定 - SimpleMemo")
        .inner_size(400.0, 250.0)
        .position(parent_pos.x, parent_pos.y) // 親ウィンドウの位置
        // .always_on_top(true)
        // .skip_taskbar(true)
        .build();
    match err {
        Ok(_window) => {
            if USE_DEV_TOOL {
                #[cfg(debug_assertions)]
                _window.open_devtools();
            }
            update_main_window_enabled(app);
        },
        Err(tauri::Error::WebviewLabelAlreadyExists(_)) => {
            warn!("[create_local_setting_window] local_setting_window is already opened.");
            update_main_window_enabled(app);
        },
        Err(_) => {
            error!("[create_local_setting_window] Create window error.");
        },
    }
}

/// 読込文字コード変更画面を作成する
fn create_reload_encoding_window(app: &tauri::AppHandle) {
    trace!("[create_reload_encoding_window]");
    let label = WINDOW_LABEL_RELOAD_ENCODING.to_string();
    let parent = app.get_webview_window(WINDOW_LABEL_MAIN.into()).unwrap();
    let parent_scale_factor = parent.scale_factor().unwrap();
    let parent_pos = parent
        .outer_position()
        .unwrap()
        .to_logical::<f64>(parent_scale_factor);
    let err = tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("reload_encoding.html".into()))
        .parent(&parent).unwrap()
        .title("読込文字コード変更 -SimpleMemo") 
        .inner_size(400.0, 200.0)
        .position(parent_pos.x, parent_pos.y) // 親ウィンドウの位置
        .build();
    match err {
        Ok(_window) => {
            if USE_DEV_TOOL {
                #[cfg(debug_assertions)]
                _window.open_devtools();
            }
            update_main_window_enabled(app);
        },
        Err(tauri::Error::WebviewLabelAlreadyExists(_)) => {
            warn!("[create_reload_encoding_window] reload_encoding_window is already opened.");
            update_main_window_enabled(app);
        },
        Err(_) => {
            error!("[create_reload_encoding_window] Create window error.");
            panic!("Create window error");
        },
    }
}

/// バージョン情報画面を作成する
fn create_version_window(app: &tauri::AppHandle) {
    trace!("[create_version_window]");
    let label = WINDOW_LABEL_VERSION.to_string();
    let parent = app.get_webview_window(WINDOW_LABEL_MAIN.into()).unwrap();
    let parent_scale_factor = parent.scale_factor().unwrap();
    let parent_pos = parent
        .outer_position()
        .unwrap()
        .to_logical::<f64>(parent_scale_factor);
    let err = tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("version_window.html".into()))
        .parent(&parent).unwrap()
        .title("バージョン情報 - SimpleMemo")
        .inner_size(400.0, 200.0)
        .position(parent_pos.x, parent_pos.y) // 親ウィンドウの位置
        .build();

    match err {
        Ok(_window) => {
            if USE_DEV_TOOL {
                #[cfg(debug_assertions)]
                _window.open_devtools();
            }
            update_main_window_enabled(app);
        },
        Err(tauri::Error::WebviewLabelAlreadyExists(_)) => {
            warn!("[create_version_window] version_window is already opened.");
            update_main_window_enabled(app);
        },
        Err(_) => {
            error!("[create_version_window] Create window error.");
            panic!("Create window error");
        },
    }
}

/** ===================================================== *
 * TAURI COMMAND
 * ===================================================== */
/// コンテキストメニューを表示する
#[tauri::command]
async fn cmd_show_context_menu(app: tauri::AppHandle) {
    trace!("[cmd_show_context_menu]");
    let w = match app.get_webview_window(WINDOW_LABEL_MAIN) {
        Some(window) => window,
        None => {
            error!("[cmd_show_context_menu] main window not found.");
            return;
        }
    };
    let handle = w.app_handle();
    let menu_clear = match MenuItem::with_id(handle, CONTEXT_MENU_ID_CLEAR, "クリア", true, None::<&str>) {
        Ok(item) => item,
        Err(err) => {
            error!("[cmd_show_context_menu] create clear item error: {:?}", err);
            return;
        }
    };
    let menu_clear_submenu = match Submenu::with_id_and_items(handle, CONTEXT_MENU_ID_CLEAR_SUBMENU, "クリア", true, &[&menu_clear]) {
        Ok(submenu) => submenu,
        Err(err) => {
            error!("[cmd_show_context_menu] create clear submenu error: {:?}", err);
            return;
        }
    };
    let menu_cut = match PredefinedMenuItem::cut(handle, Some("切り取り")) {
        Ok(item) => item,
        Err(err) => {
            error!("[cmd_show_context_menu] create cut item error: {:?}", err);
            return;
        }
    };
    let menu_copy = match PredefinedMenuItem::copy(handle, Some("コピー")) {
        Ok(item) => item,
        Err(err) => {
            error!("[cmd_show_context_menu] create copy item error: {:?}", err);
            return;
        }
    };
    let menu_paste = match PredefinedMenuItem::paste(handle, Some("貼り付け")) {
        Ok(item) => item,
        Err(err) => {
            error!("[cmd_show_context_menu] create paste item error: {:?}", err);
            return;
        }
    };

    let state = app.state::<Mutex<AppData>>();
    let state = match state.lock() {
        Ok(state) => state,
        Err(err) => {
            error!("[cmd_show_context_menu] AppData mutex poisoned. err: {:?}", err);
            return;
        }
    };
    let now_page = state.memo_manager.page_num;
    let lock_status = state.memo_manager.get_lock_status()[now_page];
    debug!("[cmd_show_context_menu] now_page: {}, lock_status: {}", now_page, lock_status);

    // https://github.com/tauri-apps/tauri/issues/7760
    let mut x = MenuBuilder::new(handle);
    if lock_status {
        x = x
            .item(&menu_copy)
            .separator();
    } else {
        x = x
            .item(&menu_cut)
            .item(&menu_copy)
            .item(&menu_paste)
            .separator();
    }
    let x = match x.check(CONTEXT_MENU_ID_ALWAYS_TOP, "常に手前に表示")
        .check(CONTEXT_MENU_ID_LOCK, "ロック")
        .item(&menu_clear_submenu)
        .separator()
        .text(CONTEXT_MENU_ID_GLOBAL_SETTING, "全体設定")
        .text(CONTEXT_MENU_ID_LOCAL_SETTING, "個別設定")
        .text(CONTEXT_MENU_ID_ENCODING, "読取文字コード変更")
        .text(CONTEXT_MENU_ID_VERSION, "バージョン情報")
        .build() {
            Ok(menu) => menu,
            Err(err) => {
                error!("[cmd_show_context_menu] build menu error: {:?}", err);
                return;
            }
        };
    
    // コンテキストメニューをロック
    if let Some(item) = x.get(CONTEXT_MENU_ID_CLEAR_SUBMENU) {
        if let Some(submenu) = item.as_submenu() {
            if let Err(err) = submenu.set_enabled(!lock_status) {
                error!("[cmd_show_context_menu] set clear submenu enabled error: {:?}", err);
            }
        }
    }
    if let Some(item) = x.get(CONTEXT_MENU_ID_LOCK) {
        if let Some(check) = item.as_check_menuitem() {
            if let Err(err) = check.set_checked(lock_status) {
                error!("[cmd_show_context_menu] set lock checked error: {:?}", err);
            }
        }
    }
    if let Some(item) = x.get(CONTEXT_MENU_ID_ALWAYS_TOP) {
        if let Some(check) = item.as_check_menuitem() {
            if let Err(err) = check.set_checked(state.memo_manager.memo_setting.top_most) {
                error!("[cmd_show_context_menu] set always-top checked error: {:?}", err);
            }
        }
    }
    match w.popup_menu(&x) {
        Ok(_) => {},
        Err(_) => {
            error!("[cmd_show_context_menu] popup_menu error.");
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SaveFilePayload {
    page_num: usize,
    filename: String,
    text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SaveResultPayload {
    save_count: u32,
    is_external_file: bool,
    page_num: usize
}

/// ファイル保存する
#[tauri::command]
async fn cmd_save_file(app: tauri::AppHandle, payload: SaveFilePayload) -> Result<SaveResultPayload, MemoError> {
    trace!("[cmd_save_file]");
    debug!("[cmd_save_file] payload: {:?}", payload);
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();
    let mut result = state.memo_manager.save(payload.page_num, payload.filename.clone(), &payload.text, false);
    let window = app.get_webview_window(WINDOW_LABEL_MAIN).unwrap();
    match result {
        Ok(_) => {},
        Err(err) => {
            match err {
                MemoError::NoFileName => {
                    warn!("[cmd_save_file] NoFileName");
                    let _ = app.dialog()
                        .message(format!("ファイル名を入力してください"))
                        .kind(MessageDialogKind::Warning)
                        .title(DIALOG_TITLE)
                        .buttons(MessageDialogButtons::Ok)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::InvalidFileName => {
                    warn!("[cmd_save_file] InvalidFileName");
                    let _ = app.dialog()
                        .message("ファイル名が不正です")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::FileExist => {
                    warn!("[cmd_save_file] FileExist");
                    let answer = app.dialog()
                        .message("ファイルが存在します。上書きしますか？")
                        .title(DIALOG_TITLE)
                        .buttons(MessageDialogButtons::OkCancel)
                        .parent(&window)
                        .blocking_show();
                    if answer == true {
                        result = state.memo_manager.save(payload.page_num, payload.filename.clone(), &payload.text, true);
                        if result.is_err() {
                            error!("[cmd_save_file] Save error after overwrite: {:?}", result.unwrap_err());
                            let _ = app.dialog()
                                .message("保存エラー\n 上書き保存に失敗しました")
                                .title(DIALOG_TITLE)
                                .kind(MessageDialogKind::Error)
                                .parent(&window)
                                .blocking_show();
                        }
                        /* TODO: ダイアログ表示中に保存先を消された場合を検討する  */
                    }
                },
                MemoError::NoEntry => {
                    warn!("[cmd_save_file] NoEntry");
                    let _ = app.dialog()
                        .message("ファイル名に不正な文字または文字列が含まれています")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::NoDirectory => {
                    warn!("[cmd_save_file] NoDirectory");
                    let _ = app.dialog()
                        .message("保存先フォルダが存在しません。再設定してください")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::Busy => {
                    warn!("[cmd_save_file] Busy");
                    let _ = app.dialog()
                        .message("ファイルが開かれています。閉じてから再試行してください")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                }
                _ => {
                    error!("[cmd_save_file] Unexpected error.");
                    let _ = app.dialog()
                        .message("保存エラー\n 想定外のエラーが発生しました")
                        .kind(MessageDialogKind::Error)
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                }
            }
        }
    }
    /* 保存結果を返す */
    match result {
        Ok(_) => {
            debug!("[cmd_save_file] Save success.");
            Ok(SaveResultPayload {
                save_count: state.memo_manager.get_memo(payload.page_num as usize).save_count,
                is_external_file: state.memo_manager.get_memo(payload.page_num as usize).is_external_file,
                page_num: payload.page_num,
            })
        },
        Err(err) => {
            debug!("[cmd_save_file] Save error. err: {:?}", err);
            Err(err)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct LoadFilePayload {
    page_num: usize,
    path: String,
}

/// ファイルを読み込む
fn load_memo(app: &AppHandle, memo_manager: &mut MemoManager, payload: LoadFilePayload, ignore_fsize: bool, overwrite: bool, encoding: Option<EncType>) -> Result<LoadedMemo, MemoError> {
    trace!("[load_memo]");
    debug!("[load_memo] payload: {:?}, ignore_fsize: {}, overwrite: {}, encoding: {:?}", payload, ignore_fsize, overwrite, encoding);
    let window = app.get_webview_window(WINDOW_LABEL_MAIN).unwrap();
    let mut memo_data = memo_manager.load(payload.page_num, payload.path.clone(), ignore_fsize, overwrite, encoding);
    match memo_data.clone() {
        Ok(_memo) => {},
        Err(err) => {
            match err {
                MemoError::AlreadyOpen => {
                    warn!("[load_memo] AlreadyOpen");
                    let _ = app.dialog()
                        .message("このメモはすでに開いています")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::LeaveMemo => {
                    warn!("[load_memo] LeaveMemo");
                    let answer = app.dialog()
                        .message("メモが残っています。開きますか？")
                        .title(DIALOG_TITLE)
                        .buttons(MessageDialogButtons::OkCancel)
                        .parent(&window)
                        .blocking_show();
                    if answer == true {
                        match clear_memo(app, memo_manager, payload.page_num) { // 未保存フラグ'*'を消すために実行
                            Ok(_) => {},
                            Err(err) => {
                                error!("[load_memo] clear_memo error: {:?}", err);
                                return Err(MemoError::Error);
                            }
                        }
                        memo_data = load_memo(app, memo_manager, payload.clone(), ignore_fsize, true, None);
                    }
                },
                MemoError::NoEntry => {
                    warn!("[load_memo] NoEntry");
                    let _ = app.dialog()
                        .message("ファイルが存在しません")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::LargeFile => {
                    warn!("[load_memo] LargeFile");
                    let answer = app.dialog()
                        .message("ファイルサイズが巨大です。アプリが不安定になる場合があります\n開きますか？")
                        .title(DIALOG_TITLE)
                        .buttons(MessageDialogButtons::OkCancel)
                        .parent(&window)
                        .blocking_show();
                    if answer == true {
                        memo_data = load_memo(app, memo_manager, payload.clone(), true, overwrite, None);
                    }
                },
                MemoError::Busy => {
                    warn!("[load_memo] Busy");
                    let _ = app.dialog()
                        .message("ファイルは使用中です。ファイルを閉じてから再試行してください")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::Decode => {
                    warn!("[load_memo] Decode");
                    let _ = app.dialog()
                        .message("対応していない文字コードです")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                _ => {
                    error!("[load_memo] Unexpected error.");
                    let _ = app.dialog()
                        .message("予期しない読み込みエラーが発生しました")
                        .kind(MessageDialogKind::Error)
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                }
            }
        },
    }
    memo_data
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LoadedMemoPayload {
    memo: LoadedMemo,
    page_num: usize,
}

/// ファイルを読み込む
#[tauri::command]
async fn cmd_load_file(app: AppHandle, payload: LoadFilePayload) -> Result<LoadedMemoPayload, MemoError> {
    trace!("[cmd_load_file]");
    debug!("[cmd_load_file] payload: {:?}", payload);
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();
    let loaded_memo = load_memo(&app, &mut state.memo_manager, payload.clone(), false, false, None);

    let ret = match loaded_memo {
        Ok(memo) => {
            Ok(LoadedMemoPayload {
                memo: memo,
                page_num: payload.page_num,
            })
        },
        Err(err) => {
            error!("[cmd_load_file] Load file error: {:?}", err);
            Err(err)
        }
    };
    ret
}

/// 全体設定ウィンドウに現在の設定を渡す
#[tauri::command]
fn cmd_get_global_setting(app: AppHandle) -> MemoSetting {
    trace!("[cmd_get_global_setting]");
    let state = app.state::<Mutex<AppData>>();
    let state = state.lock().unwrap();

    state.memo_manager.get_global_setting()
}

/// 全体設定の値を受け取り、反映する
#[tauri::command]
async fn cmd_set_global_setting(app: AppHandle, payload: MemoSetting) {
    trace!("[cmd_set_global_setting]");
    debug!("[cmd_set_global_setting] payload: {:?}", payload);
    let window = app.get_webview_window(WINDOW_LABEL_GLOBAL_SETTING).unwrap();
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();
    let ret = state.memo_manager.set_global_setting(&payload);
    if ret.is_err() {
        match ret.unwrap_err() {
            MemoError::InvalidFontSize => {
                warn!("[cmd_set_global_setting] InvalidFontSize");
                let _ = app.dialog()
                    .message("フォントサイズが不正です。\n1~100の範囲で入力してください。")
                    .title(DIALOG_TITLE)
                    .parent(&window)
                    .blocking_show();
            },
            _ => {
                error!("[cmd_set_global_setting] Unexpected error.");
                let _ = app.dialog()
                    .message("予期しないエラーが発生しました。(cmd_set_global_setting)")
                    .title(DIALOG_TITLE)
                    .parent(&window)
                    .blocking_show();
            }
        }
    } else {
        set_ui_setting(&app, &state.memo_manager.memo_setting);   // フロントエンドのUIを更新
        /* ウィンドウを閉じる */
        let _ = app.get_webview_window(WINDOW_LABEL_GLOBAL_SETTING).unwrap().close();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LocalSettingResultPayload {
    page_num: usize,
    encoding: EncType,
}

/// 個別設定ウィンドウに現在の設定を渡す
#[tauri::command]
fn cmd_get_local_setting(app: AppHandle) -> LocalSettingResultPayload {
    trace!("[cmd_get_local_setting]");
    let state = app.state::<Mutex<AppData>>();
    let state = state.lock().unwrap();
    let memo = state.memo_manager.get_memo(state.memo_manager.page_num);

    debug!("[cmd_get_local_setting] page_num: {}, encoding: {:?}", state.memo_manager.page_num, memo.encoding);
    LocalSettingResultPayload {
        page_num: state.memo_manager.page_num,
        encoding: memo.encoding,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LocalSettingPayload {
    encoding: EncType,
}

/// 個別設定の値を受け取り、反映する
#[tauri::command]
fn cmd_set_local_setting(app: AppHandle, payload: LocalSettingPayload) -> () {
    trace!("[cmd_set_local_setting]");
    debug!("[cmd_set_local_setting] payload: {:?}", payload);
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();   
    let page_num = state.memo_manager.page_num;
    let memo = state.memo_manager.get_memo_mut(page_num);
    memo.set_encoding(payload.encoding);

    let _ = app.get_webview_window(WINDOW_LABEL_LOCAL_SETTING).unwrap().close();
}

/* cmd_get_local_setting()と同じため実装しない */
// #[tauri::command]
// fn cmd_get_now_encoding(app: AppHandle) -> EncType {
//     let state = app.state::<Mutex<AppData>>();
//     let state = state.lock().unwrap();

//     let memo = state.memo_manager.get_memo(state.memo_manager.page_num);
// }

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReloadEncodingPayload {
    // page_num: usize,     // TODO: ページ番号を貰ったほうが良ければ追加する
    new_encoding: EncType,
}

/// 読込文字コードを変更する
#[tauri::command]
async fn cmd_reload_encoding(app: AppHandle, payload: ReloadEncodingPayload) -> Result<LoadedMemo, MemoError> {
    trace!("[cmd_reload_encoding]");
    debug!("[cmd_reload_encoding] payload: {:?}", payload);
    let window = match app.get_webview_window(WINDOW_LABEL_RELOAD_ENCODING) {
        Some(w) => w,
        None => {
            error!("[cmd_reload_encoding] Window 'reload_encoding_window' not found.");
            return Err(MemoError::Error);
        }
    };
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();   
    let page_num = state.memo_manager.page_num;
    let memo = state.memo_manager.get_memo(page_num);

    if memo.is_external_file == false {
        warn!("[cmd_reload_encoding] Not external file.");
        let _ = app.dialog()
            .message("外部読み込みファイルではないため実行できません")
            .title(DIALOG_TITLE)
            .parent(&window)
            .blocking_show();
        return Err(MemoError::Error);
    }

    let filename = memo.fullpath.clone();   // 初回読み込み時に設定したフルパス
    let memo_data = match state.memo_manager.load(page_num, filename, true, true, Some(payload.new_encoding)) {
        Ok(data) => {
            debug!("[cmd_reload_encoding] Load success.");
            data
        },
        Err(err) => {
            match err {
                MemoError::NoEntry => {
                    warn!("[cmd_reload_encoding] NoEntry");
                    let _ = app.dialog()
                        .message("ファイルが存在しません。\n移動、名前変更、削除された可能性があります。")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                MemoError::Busy => {
                    warn!("[cmd_reload_encoding] Busy");
                    let _ = app.dialog()
                        .message("ファイルは使用中です。ファイルを閉じてから再試行してください")
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                },
                _ => {
                    error!("[cmd_reload_encoding] Unexpected error.");
                    let _ = app.dialog()
                        .message("予期しない読み込みエラーが発生しました")
                        .kind(MessageDialogKind::Error)
                        .title(DIALOG_TITLE)
                        .parent(&window)
                        .blocking_show();
                }
            }
            return Err(err);
        }
    };

    // メモデータをメインウィンドウに渡す
    let main_window = match app.get_webview_window(WINDOW_LABEL_MAIN) {
        Some(x) => x,
        None => {
            error!("[cmd_reload_encoding] main window not found.");
            let _ = app.dialog()
                .message("予期しないエラーが発生しました")
                .kind(MessageDialogKind::Error)
                .title(DIALOG_TITLE)
                .parent(&window)
                .blocking_show();
            return Err(MemoError::Error);
        }
    };

    let ret_payload = LoadedMemoPayload {
        memo: memo_data.clone(),
        page_num: page_num
    };
    if let Err(_) = main_window.emit("load-memo", ret_payload) {
        error!("[cmd_reload_encoding] emit error.");
        let _ = app.dialog()
            .message("予期しないエラーが発生しました")
            .kind(MessageDialogKind::Error)
            .title(DIALOG_TITLE)
            .parent(&window)
            .blocking_show();
        return Err(MemoError::Error);
    }

    // reload_encoding_windowを閉じる
    match app.get_webview_window(WINDOW_LABEL_RELOAD_ENCODING) {
        Some(x) => {
            let _ = x.close();
        },
        None => {
            debug!("[cmd_reload_encoding] Window 'reload_encoding_window' is already closed.");
        }
    }

    Ok(memo_data)
}

/// 現在のページ番号を設定する
#[tauri::command]
fn cmd_set_pagenum(app: AppHandle, page_num: usize) -> () {
    trace!("[cmd_set_pagenum]");
    debug!("[cmd_set_pagenum] page_num: {}", page_num);
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();
    state.memo_manager.set_page_num(page_num);
}

/// フォントサイズを設定する
#[tauri::command]
fn cmd_set_fontsize(app: AppHandle, fontsize: u32) -> () {
    trace!("[cmd_set_fontsize]");
    debug!("[cmd_set_fontsize] fontsize: {}", fontsize);
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();
    state.memo_manager.set_font_size(fontsize);
}

/// 未保存フラグを立てる
#[tauri::command]
fn cmd_file_unsaved(app: AppHandle, page_num: usize) -> () {
    trace!("[cmd_file_unsaved]");
    debug!("[cmd_file_unsaved] page_num: {}", page_num);
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();
    state.memo_manager.set_unsaved(page_num);
}

/// ロック状態を設定する
#[tauri::command]
fn cmd_set_lock_status_main(app: AppHandle, page_num: usize) -> () {
    trace!("[cmd_set_lock_status_main]");
    debug!("[cmd_set_lock_status_main] page_num: {}", page_num);
    let state = app.state::<Mutex<AppData>>();
    let mut state = state.lock().unwrap();
    state.memo_manager.toggle_lock_status(page_num);
}

/// ロック状態をコンテキストメニューに反映する
/// 何もしない（コンテキストメニュー作成時に有効無効を判断するため）
#[tauri::command]
fn cmd_update_lock_status_main(_app: AppHandle) -> () {
    trace!("[cmd_update_lock_status_main]");
}

/// メインウィンドウの準備が完了した時に呼ばれる
#[tauri::command]
fn cmd_main_window_ready(app: AppHandle) -> () {
    trace!("[cmd_main_window_ready]");
    debug!("[cmd_main_window_ready]");
    let state = app.state::<Mutex<AppData>>();
    let state = state.lock().unwrap();

    // UI設定を反映
    set_ui_setting(&app, &state.memo_manager.memo_setting);
}


/* ===================================================== *
 * TAURI MAIN
 * ===================================================== */
/// メイン
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = resolve_app_data_dir(&app.handle())?;
            std::fs::create_dir_all(&app_data_dir)?;
            let settings_filepath = app_data_dir.join(SETTING_FILENAME);
            let settings_filepath = settings_filepath.to_string_lossy().into_owned();
            let log_filepath = app_data_dir.join(LOG_FILENAME);
            let log_level = if cfg!(debug_assertions) {
                simplelog::LevelFilter::Debug
            } else {
                simplelog::LevelFilter::Error
            };

            // ロガーの設定
            let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> = vec![
                simplelog::TermLogger::new(
                    log_level,
                    simplelog::Config::default(),
                    simplelog::TerminalMode::Mixed,
                    simplelog::ColorChoice::Auto,
                ),
            ];
            match OpenOptions::new().append(true).create(true).open(&log_filepath) {
                Ok(logfile) => {
                    loggers.push(simplelog::WriteLogger::new(
                        log_level,
                        simplelog::Config::default(),
                        logfile,
                    ));
                }
                Err(err) => {
                    eprintln!(
                        "[main] failed to open log file. path: {:?}, err: {:?}",
                        log_filepath, err
                    );
                }
            }
            if let Err(err) = simplelog::CombinedLogger::init(loggers) {
                eprintln!("[main] logger init error: {:?}", err);
            }

            let main_window = app.get_webview_window(WINDOW_LABEL_MAIN).unwrap();
            if USE_DEV_TOOL {
                #[cfg(debug_assertions)]
                main_window.open_devtools();
            }
            main_window.on_menu_event(move |window, event| {
                let handle: &AppHandle = window.app_handle();
                let state = handle.state::<Mutex<AppData>>();
                let mut state = match state.lock() {
                    Ok(state) => state,
                    Err(err) => {
                        error!("[menu_event] AppData mutex poisoned. err: {:?}", err);
                        return;
                    }
                };
                let now_page = state.memo_manager.page_num;
                debug!("[menu_event] event.id: {:?}", event.id);
                if event.id == CONTEXT_MENU_ID_LOCK {
                    debug!("[menu_event] lock");
                    state.memo_manager.toggle_lock_status(now_page);
                    match window.emit("set-lock-status", state.memo_manager.get_lock_status()) {
                        Ok(_) => {},
                        Err(_) => {
                            error!("[menu_event] 'set-lock-status' emit error.");
                        }
                    }
                }
                else if event.id == CONTEXT_MENU_ID_ALWAYS_TOP {
                    debug!("[menu_event] always-top");
                    state.memo_manager.memo_setting.top_most = !state.memo_manager.memo_setting.top_most;
                    match window.set_always_on_top(state.memo_manager.memo_setting.top_most) {
                        Ok(_) => {
                            debug!("[menu_event] set_always_on_top: {:?}", state.memo_manager.memo_setting.top_most);
                        },
                        Err(_) => {
                            error!("[menu_event] 'set_always_on_top' error.");
                        }
                    }
                }
                else if event.id == CONTEXT_MENU_ID_GLOBAL_SETTING {
                    debug!("[menu_event] global-setting");
                    create_global_setting_window(handle);
                }
                else if event.id == CONTEXT_MENU_ID_LOCAL_SETTING {
                    debug!("[menu_event] local-setting");
                    create_local_setting_window(handle);
                }
                else if event.id == CONTEXT_MENU_ID_ENCODING {
                    debug!("[menu_event] encoding");
                    create_reload_encoding_window(handle);
                }
                else if event.id == CONTEXT_MENU_ID_VERSION {
                    debug!("[menu_event] version");
                    create_version_window(handle);
                }
                else if event.id == CONTEXT_MENU_ID_CLEAR {
                    debug!("[menu_event] clear");
                    match clear_memo(handle, &mut state.memo_manager, now_page) {
                        Ok(_) => {},
                        Err(err) => {
                            error!("[menu_event] clear_memo error: {:?}", err);
                        }
                    }
                }
                else {
                    error!("[menu_event] Unknown event id.");
                }
            });

            let mut memo_manager = MemoManager::new(MAX_PAGENUM, MemoSetting::new());
            match memo_manager.load_setting(settings_filepath.clone()) {
                Ok(_) => {},
                Err(err) => {
                    // let answer = app.dialog()
                    //     .message(format!("設定ファイルが見つかりません ({:?})", err))
                    //     .title("ERROR")
                    //     .buttons(MessageDialogButtons::Ok)
                    //     .blocking_show();
                    warn!(
                        "[main] Setting file not found or invalid. path: {}, err: {:?}",
                        settings_filepath, err
                    );
                }
            }

            // UIへの設定反映(set_ui_setting)は、フロントエンド側でウィンドウロードが完了したタイミングで実行するためここでは行わない

            app.manage( Mutex::new( AppData {
                memo_manager: memo_manager,
                allow_main_close: false,
                setting_filepath: settings_filepath,
            }));
            // usage: 
            // let state = app.state::<Mutex<AppData>>();
            // let mut state = state.lock().unwrap();
            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            cmd_show_context_menu,
            cmd_save_file,
            cmd_load_file,
            cmd_get_global_setting,
            cmd_set_global_setting,
            cmd_get_local_setting,
            cmd_set_local_setting,
            cmd_reload_encoding,
            cmd_set_pagenum,
            cmd_set_fontsize,
            cmd_file_unsaved,
            cmd_set_lock_status_main,
            cmd_update_lock_status_main,
            cmd_main_window_ready,
        ])
        .on_window_event(|window, event| {
            // debug!("[main] on_window_event: {:?}", event);
            if window.label() == WINDOW_LABEL_MAIN {
                let handle = window.app_handle();
                if let tauri::WindowEvent::CloseRequested {api, ..} = event {
                    debug!("[main] CloseRequested");
                    let state = handle.state::<Mutex<AppData>>();
                    {
                        // 終了ダイアログでOKを押すと再度CloseRequestが呼ばれこのルートに入る
                        let mut state = state.lock().unwrap();
                        if state.allow_main_close {
                            state.allow_main_close = false;
                            debug!("[main] Close allowed.");
                            return;
                        }
                    }
                    let unsaved_list = {
                        let state = state.lock().unwrap();
                        state.memo_manager.get_unsaved_list()
                    };
                    info!("[main] unsaved_list: {:?}", unsaved_list);
                    let mut message = String::new();
                    if unsaved_list.len() > 0 {
                        for x in unsaved_list{
                            message += &format!("{}面", x + 1);
                        }
                        message += "が未保存です\n";
                    }
                    message += "終了しますか？";

                    api.prevent_close();
                    let app_handle = handle.clone();
                    handle.dialog()
                        .message(&message)
                        .title(DIALOG_TITLE)
                        .buttons(MessageDialogButtons::OkCancel)
                        .parent(&window)
                        .show(move |answer| {
                            if answer {
                                let state = app_handle.state::<Mutex<AppData>>();
                                {
                                    // 設定を保存して終了
                                    let mut state = state.lock().unwrap();
                                    let setting_filepath = state.setting_filepath.clone();
                                    match state.memo_manager.save_setting(setting_filepath) {
                                        Ok(_) => {}
                                        Err(err) => {
                                            error!("[main] save_setting error: {:?}", err);
                                        }
                                    }
                                    state.allow_main_close = true;
                                }
                                if let Some(main_window) = app_handle.get_webview_window(WINDOW_LABEL_MAIN) {
                                    if let Err(err) = main_window.close() {
                                        error!("[main] close error: {:?}", err);
                                    }
                                }
                            } else {
                                // アプリは終了しない
                                debug!("[main] Close canceled.");
                            }
                        });
                }
            } else if let tauri::WindowEvent::Destroyed = event {
                debug!("[{}] Destroyed", window.label());
                let handle = window.app_handle();
                update_main_window_enabled_ignoring(&handle, Some(window.label()));
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
