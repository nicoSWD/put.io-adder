//
//  PutioMainController.h
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Foundation/Foundation.h>
#import "PutioBrowser.h"
#import "V2PutIOAPIClient.h"

@interface PutioMainController : NSWindowController<NSTableViewDataSource, NSTableViewDelegate>
{
    IBOutlet NSTextField *message;
    IBOutlet NSProgressIndicator *activityIndicator;
    IBOutlet NSTextField *userInfo;
    IBOutlet NSTextField *transferInfo;
    IBOutlet NSTextField *versionInfo;
    IBOutlet NSPanel *prefSheet;
    IBOutlet NSTableView *tableView;
    IBOutlet NSButton *toggleShowTransfers;
    IBOutlet NSButton *cancelTransfer;
    
    PutioBrowser *authWindow;
    NSMutableArray *transfers;
}

- (id)init;
- (IBAction)openGithub:(id)sender;
- (IBAction)openPrefefrences:(id)sender;
- (IBAction)closePreferences:(id)sender;
- (IBAction)checkForUpdates:(id)sender;
- (IBAction)toggleShowTransfers:(id)sender;
- (IBAction)cancelTransfer:(id)sender;
- (void)openFileOnPutIO;

@property (nonatomic, retain) NSTextField *message;
@property (nonatomic, retain) NSProgressIndicator *activityIndicator;
@property (nonatomic, retain) NSTextField *userInfo;
@property (nonatomic, retain) NSTextField *transferInfo;
@property (nonatomic, retain) NSTextField *versionInfo;
@property (assign) IBOutlet NSWindow *putiowindow;
@property (nonatomic, retain) IBOutlet NSTableView *tableView;
@property (nonatomic, retain) IBOutlet NSButton *toggleShowTransfers;
@property (nonatomic, retain) IBOutlet NSButton *cancelTransfer;

@property (strong) PutioBrowser *authWindow;
@property (strong) NSMutableArray *transfers;

@end