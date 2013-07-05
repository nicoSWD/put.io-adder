//
//  PutioAppDelegate.h
//  put.io adder
//
//  Created by Nicolas Oelgart on 2/21/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Cocoa/Cocoa.h>
#import "PutioBrowser.h"

@interface PutioAppDelegate : NSObject <NSApplicationDelegate>
{
    IBOutlet NSTextField *message;
    IBOutlet NSProgressIndicator *progress;
    
    NSString *oauthToken;
    PutioBrowser *authWindow;
}

@property (assign) IBOutlet NSWindow *window;

@property (nonatomic, retain) NSTextField *message;
@property (nonatomic, retain) NSProgressIndicator *progress;

@property (nonatomic, retain) NSString *oauthToken;
@property (strong) PutioBrowser *authWindow;

@property (readonly, strong, nonatomic) NSPersistentStoreCoordinator *persistentStoreCoordinator;
@property (readonly, strong, nonatomic) NSManagedObjectModel *managedObjectModel;
@property (readonly, strong, nonatomic) NSManagedObjectContext *managedObjectContext;

- (IBAction)saveAction:(id)sender;

@end
