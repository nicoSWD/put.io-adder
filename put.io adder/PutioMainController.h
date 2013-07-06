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
    IBOutlet NSTextField *userInfo;
    
    NSString *oauthToken;
    PutioBrowser *authWindow;
    NSTimer *userInfoTimer;
}

- (void)authenticateUser;
- (void)updateUserInfo;
- (id)transformedValue:(id)value;
- (void)handleEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent;
- (IBAction)loadWebsite:(id)sender;

@property (nonatomic, retain) NSTextField *message;
@property (nonatomic, retain) NSProgressIndicator *progress;
@property (nonatomic, retain) NSProgressIndicator *waiting;
@property (nonatomic, retain) NSTextField *waitingLabel;

@property (nonatomic, retain) NSTextField *userInfo;
@property (nonatomic, retain) NSString *oauthToken;
@property (strong) PutioBrowser *authWindow;

@end