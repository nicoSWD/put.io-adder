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

@end