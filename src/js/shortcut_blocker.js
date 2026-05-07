(function () {
  const enabledFlag = '__simpleMemoShortcutBlockerEnabled';

  function isBlockedShortcut (event) {
    const key = event.key.toLowerCase();
    const hasCommandModifier = event.ctrlKey || event.metaKey;

    const isReload =
      event.key === 'F5' ||
      (hasCommandModifier && key === 'r');

    const isPrint =
      hasCommandModifier && key === 'p';

    const isFind =
      (hasCommandModifier && key === 'f') ||
      event.key === 'F3';

    const isDevTools =
      event.key === 'F12' ||
      (hasCommandModifier && event.shiftKey && key === 'i') ||
      (hasCommandModifier && event.shiftKey && key === 'c');

    const isZoom =
      hasCommandModifier &&
      ['+', '-', '=', '0'].includes(event.key);

    return isReload || isPrint || isFind || isDevTools || isZoom;
  }

  function blockShortcut (event) {
    if (isBlockedShortcut(event)) {
      event.preventDefault();
      event.stopPropagation();
    }
  }

  function enable () {
    if (window[enabledFlag] === true) {
      return;
    }

    window.addEventListener('keydown', blockShortcut, true);
    window[enabledFlag] = true;
  }

  window.SimpleMemoShortcutBlocker = Object.freeze({
    enable,
  });
}());
