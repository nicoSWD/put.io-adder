//
//  PutioAppDelegate.m
//  put.io adder
//
//  Created by Nicolas Oelgart on 2/21/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioAppDelegate.h"
#import "PutioMainController.h"
#import "PutioHelper.h"

@implementation PutioAppDelegate

- (void)applicationWillBecomeActive:(NSNotification *)notification {
    NSInteger state = ([[NSUserDefaults standardUserDefaults] boolForKey:@"CloseAfterSaving"]) ? NSOnState : NSOffState;
    [self.closeAfterSendingMenuItem setState:state];
}

- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)sender
{
    return YES;
}


- (BOOL)application:(NSApplication *)sender openFile:(NSString *)pathname
{
    if ([[[pathname pathExtension] lowercaseString] isEqualToString:@"torrent"])
    {        
        [[PutioHelper sharedHelper] uploadTorrent:pathname];
        return YES;
    }
    
    return NO;
}

- (IBAction)closeAfterSendingClicked:(id)sender {
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    [defaults setBool:![defaults boolForKey:@"CloseAfterSaving"] forKey:@"CloseAfterSaving"];
    [defaults synchronize];

    NSInteger state = ([defaults boolForKey:@"CloseAfterSaving"]) ? NSOnState : NSOffState;

    [self.closeAfterSendingMenuItem setState:state];
}


@end