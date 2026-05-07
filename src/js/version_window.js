
window.onload = function() {
    window.SimpleMemoShortcutBlocker.enable();
    document.addEventListener('contextmenu', event => {
        event.preventDefault();
    });
};
