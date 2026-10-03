((game) => {
    try {
        const url = new URL(location.href);
        const scope = game === 'majsoul'
            ? ['game.maj-soul.com', '/1/']
            : game === 'tenhou' ? ['tenhou.net', '/4/'] : null;
        return scope !== null && url.protocol === 'https:' &&
            url.hostname === scope[0] && url.port === '' &&
            url.username === '' && url.password === '' && url.pathname.startsWith(scope[1]);
    } catch {
        return false;
    }
})(__AKAGI_GAME__)
