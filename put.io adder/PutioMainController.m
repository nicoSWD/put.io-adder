//
//  PutioMainController.m
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioMainController.h"
#import "PutioHelper.h"
#import "PutioSearchResultsTableView.h"

@implementation PutioMainController

@synthesize
    message,
    activityIndicator,
    userInfo,
    transferInfo,
    putiowindow,
    transfers,
    tableView,
    diskusage,
    usageMsg,
    toggleTransfers,
    avatar,
    searchResults,
    popResults,
    scrollView;

- (id)init
{
    PutioHelper *helper = [PutioHelper sharedHelper];
    [helper setPutioController:self];
    
    return self;
}

- (void)awakeFromNib
{
    self.window.title = [NSString stringWithFormat:@"%@ v%@", self.window.title, [[[NSBundle mainBundle] infoDictionary] objectForKey:@"CFBundleShortVersionString"]];
    self.userInfo.stringValue = NSLocalizedString(@"HELPER_FETCHING_USERINFO", nil);
    self.message.stringValue = NSLocalizedString(@"HELPER_MSG_READY", nil);
    
    PutioHelper *helper = [PutioHelper sharedHelper];
    [helper authenticateUser];
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    
    // Run if "not set" or "enabled"
    if ([defaults objectForKey:@"checkupdate"] == nil || [defaults boolForKey:@"checkupdate"] == YES) {
        [helper checkForUpdates];
    }
    
    [[NSAppleEventManager sharedAppleEventManager] setEventHandler:helper andSelector:@selector(handleURLEvent:withReplyEvent:) forEventClass:kInternetEventClass andEventID:kAEGetURL];
    
    NSSortDescriptor *sortDescriptor = [[NSSortDescriptor alloc] initWithKey:@"status" ascending:YES];
    
    [self.tableView setTarget:self];
    [self.tableView setDoubleAction:@selector(openFileOnPutIO)];
    [self.tableView setSortDescriptors:[NSArray arrayWithObject:sortDescriptor]];
    
    [[[self.tableView.tableColumns objectAtIndex:0] headerCell] setTitle: NSLocalizedString(@"HELPER_TABLEHEADER_NAME", nil)];
    [[[self.tableView.tableColumns objectAtIndex:1] headerCell] setTitle: NSLocalizedString(@"HELPER_TABLEHEADER_STATUS", nil)];
}

- (IBAction)openGithub:(id)sender
{
    [[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:@"https://github.com/nicoSWD/put.io-adder"]];
}

- (IBAction)openPrefefrences:(id)sender
{
    [NSApp beginSheet:prefSheet modalForWindow:(NSWindow *)putiowindow modalDelegate:self didEndSelector:nil contextInfo:nil];
}

- (IBAction)closePreferences:(id)sender
{
    [NSApp endSheet:prefSheet];
    [prefSheet orderOut:sender];
}

- (IBAction)checkForUpdates:(id)sender
{
    [[PutioHelper sharedHelper] checkForUpdates];
}

- (bool)transfersAreVisible
{
    return self.putiowindow.frame.size.height > self.putiowindow.minSize.height;
}

- (void)toggleShowTransfers
{
    CGRect newFrame;
    int extraMargin = 106;
    int tableHeight;
    int tableWidth;
    
    if ([self transfersAreVisible]) {
        tableHeight = 0;
        tableWidth = 0;
        newFrame = CGRectMake(
            self.putiowindow.frame.origin.x,
            self.putiowindow.frame.origin.y + self.putiowindow.minSize.height + extraMargin,
            self.putiowindow.minSize.width,
            self.putiowindow.minSize.height
        );
    } else {
        tableHeight = 203;
        tableWidth = 526;
        newFrame = CGRectMake(
            self.putiowindow.frame.origin.x,
            self.putiowindow.frame.origin.y - self.putiowindow.minSize.height - extraMargin,
            self.putiowindow.maxSize.width,
            self.putiowindow.maxSize.height
        );
    }
    
    [[NSAnimationContext currentContext] setCompletionHandler:^{
        [self.scrollView setFrame:CGRectMake(12, 11, tableWidth, tableHeight)];
    }];
    
    [self.putiowindow setFrame:newFrame display:YES animate:YES];
}

- (NSInteger)numberOfRowsInTableView:(NSTableView *)tableView
{
    return [self.transfers count];
}

- (id)tableView:(NSTableView *)aTableView objectValueForTableColumn:(NSTableColumn *)tableColumn row:(NSInteger)row
{
    PKTransfer *trans = [self.transfers objectAtIndex:row];
    
    if ([tableColumn.identifier isEqualToString:@"name"]) {
        return [trans name];
    } else if ([tableColumn.identifier isEqualToString:@"speed"]) {
        if (![trans.status isEqualToString:@"DOWNLOADING"]) {
            return @"0";
        }
        
        return [[PutioHelper sharedHelper] transformedValue: [trans.downSpeed doubleValue]];
    } else if ([tableColumn.identifier isEqualToString:@"eta"]) {
        int numberOfSeconds = (int)[trans.estimatedTime integerValue];
        
        if ([trans.status isEqualToString:@"COMPLETING"]) {
            return @"Finishing";
        }
        
        if ([trans.status isEqualToString:@"COMPLETED"] || [trans.status isEqualToString:@"SEEDING"]) {
            return @"Done";
        }
        
        int seconds = numberOfSeconds % 60;
        int minutes = (numberOfSeconds / 60) % 60;
        int hours = numberOfSeconds / 3600;
        
        if (hours) {
            return [NSString stringWithFormat:@"%dh %02dm", hours, minutes];
        }

        if (minutes) {
            return [NSString stringWithFormat:@"%dm %02ds", minutes, seconds];
        }
        
        if (seconds <= 0) {
            return @"Waiting...";
        }

        return [NSString stringWithFormat:@"%ds", seconds];
    } else if ([tableColumn.identifier isEqualToString:@"size"]) {
        return [[PutioHelper sharedHelper] transformedValue: [trans.size doubleValue]];
    } else if ([tableColumn.identifier isEqualToString:@"status"]) {
        return trans.percentDone;
    }
    
    return nil;
}

- (void)tableView:(NSTableView *)_tableView sortDescriptorsDidChange: (NSArray *)oldDescriptors
{
    NSArray *newDescriptors = [_tableView sortDescriptors];
    [self.transfers sortUsingDescriptors:newDescriptors];
    [self.tableView reloadData];
}

- (void)cancelTransfer:(NSMenuItem*)sender
{
    PKTransfer *transfer;
 
    @try {
        transfer = [self.transfers objectAtIndex: sender.tag];
    } @catch (NSException *exception) {
        return;
    } @finally {}
    
    self.message.stringValue = NSLocalizedString(@"HELPER_CANCELING_DOWNLOAD", nil);
    [self.activityIndicator startAnimation:nil];
    
    PutioHelper *helper = [PutioHelper sharedHelper];
    
    [[helper putioAPI] cancelTransfer:transfer:^ {
        self.message.stringValue = NSLocalizedString(@"HELPER_DOWNLOAD_CANCELED", nil);
        [self.activityIndicator stopAnimation:nil];
        
        [helper updateUserInfo];
    } failure:^(NSError *error) {
        self.message.stringValue = NSLocalizedString(@"HELPER_CANCEL_FAILED", nil);
        [self.activityIndicator stopAnimation:nil];
    }];
}

- (void)streamVideo:(NSMenuItem *)sender
{
    PKTransfer *transfer;
    
    @try {
        transfer = [self.transfers objectAtIndex: sender.tag];
    } @catch (NSException *exception) {
        return;
    } @finally {}
    
    NSBundle *bundle = [NSBundle bundleWithPath:@"/Applications/VLC.app"];
    
    if (bundle == nil) {
        NSAlert *alert = [NSAlert
            alertWithMessageText:@"Error"
            defaultButton:@"Okay"
            alternateButton:nil
            otherButton:nil
            informativeTextWithFormat:@"Unable to find VLC.app in /Applications"
        ];
        [alert runModal];
        return;
    }
    
    self.message.stringValue = @"Streaming video...";
    
    PutioHelper *helper = [PutioHelper sharedHelper];
    NSString *url = [NSString stringWithFormat:@"https://put.io/v2/files/%@/stream?oauth_token=%@", [transfer fileID], [helper putioAPI].apiToken];
    NSArray *args = [NSArray arrayWithObjects:url, nil];
    NSTask *task = [NSTask launchedTaskWithLaunchPath:bundle.executablePath arguments:args];
    [task launch];
}

- (void)userNotificationCenter:(NSUserNotificationCenter *)center didActivateNotification:(NSUserNotification *)notification
{
    NSString *url = [NSString stringWithFormat:@"https://put.io/file/%@", [notification.userInfo valueForKey:@"fileID"]];
    [[NSWorkspace sharedWorkspace] openURL: [NSURL URLWithString: url]];
}

- (void)openFileOnPutIO
{
    if (self.tableView.clickedRow == -1) {
        // Table header was clicked
        return;
    }
    
    PKTransfer *trans = [self.transfers objectAtIndex: [self.tableView clickedRow]];
    NSString *fileID = [trans fileID];
    NSString *url;
    
    if (![[trans fileID] isEqualTo:[NSNull alloc]] && fileID != nil) {
        url = [NSString stringWithFormat:@"https://put.io/file/%@", fileID];
    } else {
        url = @"https://put.io/transfers";
    }
    
    [[NSWorkspace sharedWorkspace] openURL: [NSURL URLWithString: url]];
}

@end