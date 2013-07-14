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

@interface PutioHelper : NSObject
{
    PutioMainController *putioController;
    V2PutIOAPIClient *putioAPI;
    NSTimer *userInfoTimer;
}

+ (PutioHelper*)sharedHelper;
- (id)init;
- (void)authenticateUser;
- (void)updateUserInfo;
- (void)addMagnet:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent;
- (void)uploadTorrent:(NSString*)filePath;
- (NSString*)transformedValue:(id)value;

@property (strong) PutioMainController *putioController;
@property (strong) V2PutIOAPIClient *putioAPI;

@end