//
//  PutioMainController.h
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Foundation/Foundation.h>
#import "V2PutIOAPIClient.h"
#import "PutioSearchResultsTableView.h"

@interface PutioMainController : NSWindowController<NSTableViewDataSource, NSTableViewDelegate, NSUserNotificationCenterDelegate, NSMenuDelegate>
{
    IBOutlet NSTextField *message;
    IBOutlet NSProgressIndicator *activityIndicator;
    IBOutlet NSTextField *userInfo;
    IBOutlet NSTextField *transferInfo;
    IBOutlet NSPanel *prefSheet;
    IBOutlet NSTableView *tableView;
    IBOutlet NSView *diskusage;
    IBOutlet NSTextField *usageMsg;
    IBOutlet NSImageView *toggleTransfers;
    IBOutlet NSImageView *avatar;
    IBOutlet PutioSearchResultsTableView *searchResults;
    
    NSMutableArray *transfers;
}

- (id)init;
- (IBAction)openGithub:(id)sender;
- (IBAction)openPrefefrences:(id)sender;
- (IBAction)closePreferences:(id)sender;
- (IBAction)checkForUpdates:(id)sender;
- (void)toggleShowTransfers;
- (bool)transfersAreVisible;
- (void)streamVideo:(NSMenuItem*)sender;
- (void)cancelTransfer:(NSMenuItem*)sender;
- (void)openFileOnPutIO;

@property (nonatomic, retain) NSTextField *message;
@property (nonatomic, retain) NSProgressIndicator *activityIndicator;
@property (nonatomic, retain) NSTextField *userInfo;
@property (nonatomic, retain) NSTextField *transferInfo;
@property (assign) IBOutlet NSWindow *putiowindow;
@property (nonatomic, retain) IBOutlet NSTableView *tableView;
@property (strong, nonatomic) IBOutlet NSView *diskusage;
@property (strong, nonatomic) IBOutlet NSTextField *usageMsg;
@property (strong, nonatomic) IBOutlet NSImageView *toggleTransfers;
@property (strong, nonatomic) IBOutlet NSImageView *avatar;
@property (strong, nonatomic) PutioSearchResultsTableView *searchResults;
@property (strong, nonatomic) IBOutlet NSPopover *popResults;
@property (strong) NSMutableArray *transfers;

@end