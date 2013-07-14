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
    transferInfo;


- (void)awakeFromNib
{
    self.versionInfo.stringValue = [NSString stringWithFormat:@"v%@", [[[NSBundle mainBundle] infoDictionary] objectForKey:@"CFBundleShortVersionString"]];

    [self.window setContentBorderThickness:24.0 forEdge:NSMinYEdge];

    PutioHelper *helper = [PutioHelper sharedHelper];
    [helper authenticateUser];
    
    [[NSAppleEventManager sharedAppleEventManager] setEventHandler:helper andSelector:@selector(addMagnet:withReplyEvent:) forEventClass:kInternetEventClass andEventID:kAEGetURL];
}


- (IBAction)loadWebsite:(id)sender
{
    [[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:@"https://github.com/nicoSWD/put.io-adder"]];
}

@end