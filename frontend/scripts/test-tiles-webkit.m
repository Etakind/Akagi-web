// Disposable macOS WebKit fixture. No persistent website data or game page.
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>
int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 3) return 2;
        NSApplication *app = NSApplication.sharedApplication;
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];
        WKWebViewConfiguration *config = [WKWebViewConfiguration new];
        config.websiteDataStore = WKWebsiteDataStore.nonPersistentDataStore;
        WKWebView *view = [[WKWebView alloc] initWithFrame:NSMakeRect(0, 0, 1100, 500) configuration:config];
        NSWindow *window = [[NSWindow alloc] initWithContentRect:view.frame styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
        window.contentView = view;
        BOOL blocked = strcmp(argv[2], "blocked") == 0;
        NSString *condition = blocked
            ? @"e.hasAttribute('data-render-error') && e.shadowRoot.querySelector('img').alt === '⚠'"
            : @"e.shadowRoot.querySelector('img').naturalWidth > 0 && !e.hasAttribute('data-render-error')";
        NSString *expression = [NSString stringWithFormat:@"window.fixtureReady === true && document.querySelectorAll('akagi-tiles').length === 4 && [...document.querySelectorAll('akagi-tiles')].every(e => %@)", condition];
        NSDate *start = NSDate.date;
        __block BOOL evaluating = NO;
        [NSTimer scheduledTimerWithTimeInterval:0.2 repeats:YES block:^(NSTimer *timer) {
            if (-start.timeIntervalSinceNow > 25) { puts("FAIL: native WebKit tile fixture timed out"); exit(1); }
            if (evaluating) return;
            evaluating = YES;
            [view evaluateJavaScript:expression completionHandler:^(id result, NSError *error) {
                evaluating = NO;
                if ([result respondsToSelector:@selector(boolValue)] && [result boolValue]) {
                    puts(blocked ? "PASS: WebKit blocked-worker fallback" : "PASS: WebKit production CSP tile rendering");
                    exit(0);
                }
            }];
        }];
        [view loadRequest:[NSURLRequest requestWithURL:[NSURL URLWithString:@(argv[1])]]];
        [window orderBack:nil];
        [app run];
    }
    return 1;
}
