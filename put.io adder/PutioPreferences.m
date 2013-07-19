//
//  PutioPreferencesWindowController.m
//  put.io adder
//
//  Created by Nico on 7/16/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioPreferences.h"


@implementation PutioPreferences

@synthesize
    closeWhenFinishedMagnet,
    closeWhenFinishedTorrent,
    checkForUpdates,
    closeButton,
    labelAutoCloseWhen;


- (void)awakeFromNib
{
    self.labelAutoCloseWhen.stringValue = NSLocalizedString(@"PANEL_AUTO_CLOSE_WHEN", nil);
    self.closeWhenFinishedMagnet.title = NSLocalizedString(@"PANEL_ADDING_MAGNET", nil);
    self.closeWhenFinishedTorrent.title = NSLocalizedString(@"PANEL_ADDING_TORRENT", nil);
    self.checkForUpdates.title = NSLocalizedString(@"PANEL_CHECK_FOR_UPDATES", nil);
    self.closeButton.title = NSLocalizedString(@"PANEL_CLOSE", nil);
}


- (void)becomeKeyWindow
{
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];

    self.closeWhenFinishedTorrent.state = [defaults boolForKey:@"close.torrent"];
    self.closeWhenFinishedMagnet.state = [defaults boolForKey:@"close.magnet"];
    self.checkForUpdates.state = [defaults boolForKey:@"checkupdate"];
}


- (IBAction)toggleSetting:(NSButton*)sender
{
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    [defaults setBool:(BOOL)sender.state forKey:(NSString *)sender.identifier];
    [defaults synchronize];
}

@end