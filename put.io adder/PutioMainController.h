//
//  PutioMainController.h
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Foundation/Foundation.h>
#import "PutioBrowser.h"

@interface PutioMainController : NSWindowController
{
    IBOutlet NSTextField *message;
    IBOutlet NSProgressIndicator *progress;
    IBOutlet NSProgressIndicator *waiting;
    IBOutlet NSTextField *waitingLabel;
    
    NSString *oauthToken;
    PutioBrowser *authWindow;
}

- (void)authenticateUser;
- (void)handleEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent;

@property (nonatomic, retain) NSTextField *message;
@property (nonatomic, retain) NSProgressIndicator *progress;
@property (nonatomic, retain) NSProgressIndicator *waiting;
@property (nonatomic, retain) NSTextField *waitingLabel;

@property (nonatomic, retain) NSString *oauthToken;
@property (strong) PutioBrowser *authWindow;

@end