(() => {
    const registry = window.__akagiRecoveryV1;
    if (!registry) return 'unidentified';
    const games = [...registry.games].filter(socket => socket.readyState === WebSocket.OPEN);
    if (games.length > 1) return 'ambiguous';
    if (games.length !== 1) return 'unidentified';
    games[0].close(4000, 'Akagi recovery');
    return 'closed';
})()
