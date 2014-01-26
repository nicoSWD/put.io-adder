//
//  PutioMainController.m
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioMainController.h"
#import "PutioHelper.h"

@implementation PutioMainController

@synthesize
    message,
    activityIndicator,
    authWindow,
    userInfo,
    versionInfo,
    transferInfo,
    putiowindow,
    transfers,
    tableView,
    toggleShowTransfers,
    cancelTransfer,
    transfersMenu;

static BOOL transfersAreHidden = YES;


- (id)init
{
    PutioHelper *helper = [PutioHelper sharedHelper];
    [helper setPutioController:self];
    
    return self;
}

- (void)awakeFromNib
{
    self.versionInfo.stringValue = [NSString stringWithFormat:@"v%@", [[[NSBundle mainBundle] infoDictionary] objectForKey:@"CFBundleShortVersionString"]];
    self.toggleShowTransfers.title = NSLocalizedString(@"HELPER_TRANSFERS_SHOW", nil);
    self.userInfo.stringValue = NSLocalizedString(@"HELPER_FETCHING_USERINFO", nil);
    self.message.stringValue = NSLocalizedString(@"HELPER_MSG_READY", nil);
    self.cancelTransfer.stringValue = NSLocalizedString(@"HELPER_CANCEL", nil);
    
    PutioHelper *helper = [PutioHelper sharedHelper];
    [helper authenticateUser];
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    
    // Run if "not set" or "enabled"
    if ([defaults objectForKey:@"checkupdate"] == nil || [defaults boolForKey:@"checkupdate"] == YES)
    {
        [helper checkForUpdates];
    }
    
    [[NSAppleEventManager sharedAppleEventManager] setEventHandler:helper andSelector:@selector(addMagnet:withReplyEvent:) forEventClass:kInternetEventClass andEventID:kAEGetURL];
    
    NSSortDescriptor *sortDescriptor = [[NSSortDescriptor alloc] initWithKey:@"status" ascending:YES];
    [self.window setContentBorderThickness:24.0 forEdge:NSMinYEdge];
    [self.tableView setTarget:self];
    [self.tableView setDoubleAction:@selector(openFileOnPutIO)];
    [self.tableView setSortDescriptors:[NSArray arrayWithObject:sortDescriptor]];
    
    [[[self.tableView.tableColumns objectAtIndex:0] headerCell] setTitle: NSLocalizedString(@"HELPER_TABLEHEADER_NAME", nil)];
    [[[self.tableView.tableColumns objectAtIndex:1] headerCell] setTitle: NSLocalizedString(@"HELPER_TABLEHEADER_STATUS", nil)];
    
    /*
     if ([defaults boolForKey:@"showtransfers"])
     {
     
     }
     */
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


- (IBAction)toggleShowTransfers:(id)sender
{
    [[[self.tableView superview] superview] setHidden:!transfersAreHidden];
    
    float Y = transfersAreHidden ? 100 : -100;
    float X = 0;
    
    NSRect frame = [self.putiowindow frame];
    frame.origin.y -= Y;
    frame.size.height += Y;
    frame.size.width += X;
    
    if (!transfersAreHidden)
    {
        self.toggleShowTransfers.title = NSLocalizedString(@"HELPER_TRANSFERS_SHOW", nil);
    }
    else
    {
        self.toggleShowTransfers.title = NSLocalizedString(@"HELPER_TRANSFERS_HIDE", nil);
    }
    
    /*
     NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
     [defaults setBool:transfersAreHidden forKey:@"showtransfers"];
     [defaults synchronize];
     */
    transfersAreHidden = !transfersAreHidden;
    [self.putiowindow setFrame:frame display:YES animate:YES];
}


- (NSInteger)numberOfRowsInTableView:(NSTableView *)tableView
{
    return [self.transfers count];
}


- (id)tableView:(NSTableView *)tableView objectValueForTableColumn:(NSTableColumn *)tableColumn row:(NSInteger)row
{
    PKTransfer *trans = [self.transfers objectAtIndex:row];
    
    if ([tableColumn.identifier isEqualToString:@"name"])
    {
        return [trans name];
    }
    else if ([tableColumn.identifier isEqualToString:@"status"])
    {
        if ([trans.status isEqualToString:@"COMPLETED"])
        {
            return NSLocalizedString(@"HELPER_STATE_COMPLETED", nil);
        }
        else if ([trans.status isEqualToString:@"WAITING"])
        {
            return NSLocalizedString(@"HELPER_STATE_WAITING", nil);
        }
        else if ([trans.status isEqualToString:@"IN_QUEUE"])
        {
            return NSLocalizedString(@"HELPER_STATE_QUEDED", nil);
        }
        else if ([trans.status isEqualToString:@"DOWNLOADING"])
        {
            return [NSString stringWithFormat:NSLocalizedString(@"HELPER_STATE_DOWNLOADING", nil), trans.percentDone];
        }
        else if ([trans.status isEqualToString:@"COMPLETING"])
        {
            return NSLocalizedString(@"HELPER_STATE_COMPLETING", nil);
        }
        else if ([trans.status isEqualToString:@"SEEDING"])
        {
            return NSLocalizedString(@"HELPER_STATE_SEEDING", nil);
        }
        
        return trans.status;
    }
    
    return nil;
}


- (void)tableView:(NSTableView *)_tableView sortDescriptorsDidChange: (NSArray *)oldDescriptors
{
    NSArray *newDescriptors = [_tableView sortDescriptors];
    [self.transfers sortUsingDescriptors:newDescriptors];
    [self.tableView reloadData];
}


- (void)tableViewSelectionDidChange:(NSNotification *)notification
{
    [self.cancelTransfer setHidden: (self.tableView.selectedRow > -1 ? NO : YES)];
}


- (IBAction)cancelTransfer:(id)sender
{
    if (self.tableView.selectedRow == -1)
    {
        return;
    }
    
    PKTransfer *transfer;
    
    @try
    {
        transfer = [self.transfers objectAtIndex: [self.tableView selectedRow]];
    }
    @catch (NSException *exception)
    {
        return;
    }
    @finally {}
    
    self.message.stringValue = NSLocalizedString(@"HELPER_CANCELING_DOWNLOAD", nil);
    [self.activityIndicator startAnimation:nil];
    [self.cancelTransfer setEnabled:NO];
    
    PutioHelper *helper = [PutioHelper sharedHelper];
    
    [[helper putioAPI] cancelTransfer:transfer:^
    {
        self.message.stringValue = NSLocalizedString(@"HELPER_DOWNLOAD_CANCELED", nil);
        [self.activityIndicator stopAnimation:nil];
        [self.cancelTransfer setEnabled:YES];
        [self.cancelTransfer setHidden:YES];
        
        [helper updateUserInfo];
    }
    failure:^(NSError *error)
    {
        self.message.stringValue = NSLocalizedString(@"HELPER_CANCEL_FAILED", nil);
        [self.activityIndicator stopAnimation:nil];
        [self.cancelTransfer setEnabled:YES];
        [self.cancelTransfer setHidden:YES];
    }];
}


- (void)userNotificationCenter:(NSUserNotificationCenter *)center didActivateNotification:(NSUserNotification *)notification
{
    NSString *url = [NSString stringWithFormat:@"https://put.io/file/%@", [notification.userInfo valueForKey:@"fileID"]];
    [[NSWorkspace sharedWorkspace] openURL: [NSURL URLWithString: url]];
}


- (void)openFileOnPutIO
{
    // Table header was clicked
    if (self.tableView.clickedRow == -1)
    {
        return;
    }
    
    PKTransfer *trans = [self.transfers objectAtIndex: [self.tableView clickedRow]];
    NSString *fileID = [trans fileID];
    NSString *url;
    
    if (![[trans fileID] isEqualTo:[NSNull alloc]] && fileID != nil)
    {
        url = [NSString stringWithFormat:@"https://put.io/file/%@", fileID];
    }
    else
    {
        url = @"https://put.io/transfers";
    }
    
    [[NSWorkspace sharedWorkspace] openURL: [NSURL URLWithString: url]];
}

@end