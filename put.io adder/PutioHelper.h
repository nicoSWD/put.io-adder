//
//  PutioHelper.h
//  put.io adder
//
//  Created by Nico on 7/13/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Foundation/Foundation.h>
#import "PutioMainController.h"
#import "V2PutIOAPIClient.h"

@interface PutioHelper : NSObject <NSAlertDelegate>
{
    PutioMainController *putioController;
    V2PutIOAPIClient *putioAPI;
    NSTimer *userInfoTimer;
    NSTimer *closeTimer;
}

+ (PutioHelper *)sharedHelper;
- (void)authenticateUser;
- (void)startUserinfoTimer;
- (void)updateUserInfo;
- (void)handleURLEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent;
- (void)addMagnet:(NSString *)magnetURL;
- (void)uploadTorrent:(NSString *)filePath;
- (void)checkForUpdates;
- (NSString *)transformedValue:(id)value;

@property (strong) PutioMainController *putioController;
@property (strong) V2PutIOAPIClient *putioAPI;

@end