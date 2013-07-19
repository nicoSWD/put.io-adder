//
//  PutioPreferencesWindowController.h
//  put.io adder
//
//  Created by Nico on 7/16/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Cocoa/Cocoa.h>

@interface PutioPreferences : NSPanel
{
    IBOutlet NSButton *closeWhenFinishedMagnet;
    IBOutlet NSButton *closeWhenFinishedTorrent;
    IBOutlet NSButton *checkForUpdates;
    IBOutlet NSButton *closeButton;
    IBOutlet NSTextField *labelAutoCloseWhen;
}

- (IBAction)toggleSetting:(NSButton*)sender;

@property (retain, nonatomic) NSButton *closeWhenFinishedMagnet;
@property (retain, nonatomic) NSButton *closeWhenFinishedTorrent;
@property (retain, nonatomic) NSButton *checkForUpdates;
@property (retain, nonatomic) NSButton *closeButton;
@property (retain, nonatomic) NSTextField *labelAutoCloseWhen;

@end