//
//  PutioAppDelegate.m
//  put.io adder
//
//  Created by Nicolas Oelgart on 2/21/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioAppDelegate.h"
#import "PutioMainController.h"


@implementation PutioAppDelegate


- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)sender
{
    return YES;
}


- (BOOL)application:(NSApplication *)sender openFile:(NSString *)pathname
{
    if ([[pathname pathExtension] isEqualToString:@"torrent"])
    {
        PutioMainController *putio = [[[[NSApplication sharedApplication] windows] objectAtIndex:0] windowController];
        
        [putio uploadTorrent:pathname];
        return YES;
    }
    
    return NO;
}

@end