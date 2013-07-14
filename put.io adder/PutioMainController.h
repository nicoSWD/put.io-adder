//
//  PutioMainController.h
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Foundation/Foundation.h>
#import "PutioBrowser.h"
#import "V2PutIOAPIClient.h"

@interface PutioMainController : NSWindowController
{
    IBOutlet NSTextField *message;
    IBOutlet NSProgressIndicator *activityIndicator;
    IBOutlet NSTextField *userInfo;
    IBOutlet NSTextField *transferInfo;
    IBOutlet NSTextField *versionInfo;
    
    PutioBrowser *authWindow;
}

- (IBAction)loadWebsite:(id)sender;

@property (nonatomic, retain) NSTextField *message;
@property (nonatomic, retain) NSProgressIndicator *activityIndicator;
@property (nonatomic, retain) NSTextField *userInfo;
@property (nonatomic, retain) NSTextField *transferInfo;
@property (nonatomic, retain) NSTextField *versionInfo;
@property (strong) PutioBrowser *authWindow;
@end