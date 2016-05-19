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
//#import <QTKit/QTKit.h>

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
    popResults;

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
    
    if ([self transfersAreVisible]) {
        newFrame = CGRectMake(
            self.putiowindow.frame.origin.x,
            self.putiowindow.frame.origin.y + self.putiowindow.minSize.height + extraMargin,
            self.putiowindow.minSize.width,
            self.putiowindow.minSize.height
        );
    } else {
        newFrame = CGRectMake(
            self.putiowindow.frame.origin.x,
            self.putiowindow.frame.origin.y - self.putiowindow.minSize.height - extraMargin,
            self.putiowindow.maxSize.width,
            self.putiowindow.maxSize.height
        );
        
        [self.putiowindow setShowsResizeIndicator:YES];
    }
    
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
        
        if (![trans.status isEqualToString:@"DOWNLOADING"] || numberOfSeconds <= 0) {
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


- (void)tableView:(NSTableView *)aTableView willDisplayCell:(id)aCell forTableColumn:(NSTableColumn *)aTableColumn row:(NSInteger)rowIndex
{
//    if ([aCell respondsToSelector:@selector(setBackgroundColor:)]) {
//        if ([[aTableView selectedRowIndexes] containsIndex:rowIndex])
//        {
//            [aCell setBackgroundColor: [NSColor yellowColor]];
//        }
//        else
//        {
//            [aCell setBackgroundColor: [NSColor yellowColor]];
//        }
//        
//        [aCell setDrawsBackground:YES];
//    }
}


- (void)tableViewSelectionDidChange:(NSNotification *)aNotification {
    
//    NSInteger selectedRow = [self.tableView selectedRow];
//    NSTableRowView *aCell = [self.tableView rowViewAtRow:selectedRow makeIfNecessary:NO];
//    
//    if ([[self.tableView selectedRowIndexes] containsIndex:selectedRow])
//    {
//        [aCell setBackgroundColor: [NSColor yellowColor]];
//    }
//    else
//    {
//        [aCell setBackgroundColor: [NSColor yellowColor]];
//    }
    
//    [aCell setDrawsBackground:YES];
//    [myRowView setEmphasized:NO];
}


- (void)cancelTransfer:(NSMenuItem*)sender {
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

- (void)streamVideo:(NSMenuItem *)sender {
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
    
    NSArray *args = [NSArray arrayWithObjects:@"vvv", @"https://put.io/v2/files/312186790/mp4/stream?token=", nil];
    NSTask *task = [NSTask launchedTaskWithLaunchPath:bundle.executablePath arguments:args];
    
    while (task.isRunning) {
        
    }
    
    self.message.stringValue = @"Finished";
}

- (void)userNotificationCenter:(NSUserNotificationCenter *)center didActivateNotification:(NSUserNotification *)notification {
    NSString *url = [NSString stringWithFormat:@"https://put.io/file/%@", [notification.userInfo valueForKey:@"fileID"]];
    [[NSWorkspace sharedWorkspace] openURL: [NSURL URLWithString: url]];
}


- (void)openFileOnPutIO {
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